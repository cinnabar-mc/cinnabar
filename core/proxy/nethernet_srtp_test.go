package proxy

import (
	"bytes"
	"context"
	"crypto"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/pion/datachannel"
	"github.com/pion/dtls/v3"
	"github.com/pion/dtls/v3/pkg/crypto/fingerprint"
	"github.com/pion/dtls/v3/pkg/crypto/selfsign"
	"github.com/pion/ice/v4"
	"github.com/pion/logging"
	"github.com/pion/sctp"
	"github.com/pion/sdp/v3"
	"github.com/pion/webrtc/v4"
)

// A NetherNet server whose DTLS client offers no use_srtp extension, as data-channel-only
// servers may, still completes the join handshake and carries packets: data channels need no
// SRTP, and vanilla's libwebrtc only logs the missing crypto suite.
func TestNetherNetDialConnectsWhenServerOffersNoSRTP(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()

	signaling := newOneServerSignaling(ctx)
	received := make(chan []byte, 1)
	serverErr := make(chan error, 1)
	go func() { serverErr <- serveNoSRTPNetherNet(ctx, signaling, received) }()

	dialer := remoteServerNetwork(slog.New(slog.DiscardHandler), nil, nil).NetherNet.Dialer
	conn, err := dialer.DialContext(ctx, oneServerNetworkID, signaling)
	if err != nil {
		t.Fatalf("NetherNet dial to a server without SRTP: %v", err)
	}
	defer conn.Close()

	payload := []byte("request network settings")
	if _, err := conn.Write(payload); err != nil {
		t.Fatalf("write over the reliable channel: %v", err)
	}
	select {
	case got := <-received:
		if !bytes.Equal(got, payload) {
			t.Fatalf("server received %q, want %q", got, payload)
		}
	case err := <-serverErr:
		t.Fatalf("server: %v", err)
	case <-ctx.Done():
		t.Fatal("server received no packet")
	}
}

const oneServerNetworkID = "server"

// serveNoSRTPNetherNet answers one offer like a data-channel-only NetherNet server: it takes the
// DTLS client role without SRTP profiles, then forwards the first reliable-channel segment.
func serveNoSRTPNetherNet(ctx context.Context, signaling *oneServerSignaling, received chan<- []byte) error {
	var offer *nethernet.Signal
	select {
	case offer = <-signaling.offers:
	case <-ctx.Done():
		return ctx.Err()
	}
	parsed := &sdp.SessionDescription{}
	if err := parsed.UnmarshalString(offer.Data); err != nil {
		return fmt.Errorf("decode offer: %w", err)
	}
	remote, err := nethernet.ParseDescription(parsed)
	if err != nil {
		return fmt.Errorf("parse offer: %w", err)
	}

	agent, err := ice.NewAgentWithOptions(
		ice.WithNetworkTypes([]ice.NetworkType{ice.NetworkTypeUDP4}),
		ice.WithCandidateTypes([]ice.CandidateType{ice.CandidateTypeHost}),
		ice.WithIncludeLoopback(),
	)
	if err != nil {
		return fmt.Errorf("create ICE agent: %w", err)
	}
	defer agent.Close()
	candidates, err := gatherHostCandidates(ctx, agent)
	if err != nil {
		return err
	}
	ufrag, pwd, err := agent.GetLocalUserCredentials()
	if err != nil {
		return fmt.Errorf("local ICE credentials: %w", err)
	}
	certificate, err := selfsign.GenerateSelfSigned()
	if err != nil {
		return fmt.Errorf("generate certificate: %w", err)
	}
	answer, err := noSRTPAnswer(certificate, ufrag, pwd, candidates)
	if err != nil {
		return err
	}
	signaling.deliver(&nethernet.Signal{
		Type:         nethernet.SignalTypeAnswer,
		ConnectionID: offer.ConnectionID,
		Data:         string(answer),
		NetworkID:    oneServerNetworkID,
	})

	iceConn, err := agent.Accept(ctx, remote.ICE.UsernameFragment, remote.ICE.Password)
	if err != nil {
		return fmt.Errorf("ICE accept: %w", err)
	}
	// No dtls.WithSRTPProtectionProfiles: the ClientHello carries no use_srtp extension.
	dtlsConn, err := dtls.ClientWithOptions(datagramConn{iceConn}, iceConn.RemoteAddr(),
		dtls.WithCertificates(certificate), dtls.WithInsecureSkipVerify(true))
	if err != nil {
		return fmt.Errorf("DTLS client: %w", err)
	}
	defer dtlsConn.Close()
	if err := dtlsConn.HandshakeContext(ctx); err != nil {
		return fmt.Errorf("DTLS handshake: %w", err)
	}
	loggers := logging.NewDefaultLoggerFactory()
	association, err := sctp.Client(sctp.Config{NetConn: dtlsConn, LoggerFactory: loggers})
	if err != nil {
		return fmt.Errorf("SCTP association: %w", err)
	}
	defer association.Close()
	for {
		channel, err := datachannel.Accept(association, &datachannel.Config{LoggerFactory: loggers})
		if err != nil {
			return fmt.Errorf("accept data channel: %w", err)
		}
		if channel.Config.Label != nethernet.MessageReliabilityReliable.Parameters().Label {
			continue
		}
		segment := make([]byte, 1024)
		n, err := channel.Read(segment)
		if err != nil {
			return fmt.Errorf("read reliable channel: %w", err)
		}
		// Each segment starts with its remaining-segment count.
		if n == 0 || segment[0] != 0 {
			return fmt.Errorf("unexpected segment % x", segment[:n])
		}
		received <- segment[1:n]
		return nil
	}
}

// gatherHostCandidates returns the agent's complete set of local host candidates.
func gatherHostCandidates(ctx context.Context, agent *ice.Agent) ([]webrtc.ICECandidate, error) {
	var (
		mu         sync.Mutex
		candidates []webrtc.ICECandidate
		done       = make(chan struct{})
	)
	if err := agent.OnCandidate(func(candidate ice.Candidate) {
		if candidate == nil {
			close(done)
			return
		}
		mu.Lock()
		defer mu.Unlock()
		candidates = append(candidates, webrtc.ICECandidate{
			Foundation: candidate.Foundation(),
			Priority:   candidate.Priority(),
			Address:    candidate.Address(),
			Protocol:   webrtc.ICEProtocolUDP,
			Port:       uint16(candidate.Port()),
			Typ:        webrtc.ICECandidateTypeHost,
			Component:  candidate.Component(),
		})
	}); err != nil {
		return nil, fmt.Errorf("register candidate handler: %w", err)
	}
	if err := agent.GatherCandidates(); err != nil {
		return nil, fmt.Errorf("gather candidates: %w", err)
	}
	select {
	case <-done:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	mu.Lock()
	defer mu.Unlock()
	if len(candidates) == 0 {
		return nil, errors.New("no host candidates")
	}
	return candidates, nil
}

// noSRTPAnswer encodes an answer that takes the DTLS client role with the given certificate.
func noSRTPAnswer(certificate tls.Certificate, ufrag, pwd string, candidates []webrtc.ICECandidate) ([]byte, error) {
	leaf, err := x509.ParseCertificate(certificate.Certificate[0])
	if err != nil {
		return nil, fmt.Errorf("parse certificate: %w", err)
	}
	value, err := fingerprint.Fingerprint(leaf, crypto.SHA256)
	if err != nil {
		return nil, fmt.Errorf("certificate fingerprint: %w", err)
	}
	return nethernet.Description{
		ICE: webrtc.ICEParameters{UsernameFragment: ufrag, Password: pwd},
		DTLS: webrtc.DTLSParameters{
			Role:         webrtc.DTLSRoleClient,
			Fingerprints: []webrtc.DTLSFingerprint{{Algorithm: "sha-256", Value: value}},
		},
		Candidates: candidates,
	}.Encode()
}

// datagramConn presents a connected ICE conn as the packet conn pion's DTLS client takes.
type datagramConn struct{ net.Conn }

// ReadFrom reads one datagram, which always comes from the connected peer.
func (c datagramConn) ReadFrom(b []byte) (int, net.Addr, error) {
	n, err := c.Read(b)
	return n, c.RemoteAddr(), err
}

// WriteTo writes one datagram to the connected peer, whatever the address.
func (c datagramConn) WriteTo(b []byte, _ net.Addr) (int, error) {
	return c.Write(b)
}

// oneServerSignaling connects one dialer to one in-process server: the dialer's offer goes to the
// server, and the server's signals reach the dialer's notifiers.
type oneServerSignaling struct {
	ctx    context.Context
	offers chan *nethernet.Signal

	mu        sync.Mutex
	notifiers map[int]nethernet.Notifier
	next      int
}

// newOneServerSignaling returns signaling that lives as long as ctx.
func newOneServerSignaling(ctx context.Context) *oneServerSignaling {
	return &oneServerSignaling{ctx: ctx, offers: make(chan *nethernet.Signal, 1), notifiers: map[int]nethernet.Notifier{}}
}

// Signal passes the offer to the server and drops trickled candidates, which the server learns
// from connectivity checks instead.
func (s *oneServerSignaling) Signal(ctx context.Context, signal *nethernet.Signal) error {
	if signal.Type != nethernet.SignalTypeOffer {
		return nil
	}
	select {
	case s.offers <- signal:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// deliver hands a server signal to every registered notifier.
func (s *oneServerSignaling) deliver(signal *nethernet.Signal) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, notifier := range s.notifiers {
		notifier.NotifySignal(signal)
	}
}

// Notify registers n until the returned stop function runs.
func (s *oneServerSignaling) Notify(n nethernet.Notifier) func() {
	s.mu.Lock()
	defer s.mu.Unlock()
	id := s.next
	s.next++
	s.notifiers[id] = n
	return func() {
		s.mu.Lock()
		defer s.mu.Unlock()
		delete(s.notifiers, id)
	}
}

// Context is the signaling lifetime.
func (s *oneServerSignaling) Context() context.Context { return s.ctx }

// Credentials reports no ICE servers, so only host candidates are gathered.
func (*oneServerSignaling) Credentials(context.Context) (*nethernet.Credentials, error) {
	return nil, nil
}

// NetworkID is the dialer's network ID.
func (*oneServerSignaling) NetworkID() string { return "client" }

// PongData is unused by a dialer.
func (*oneServerSignaling) PongData([]byte) {}
