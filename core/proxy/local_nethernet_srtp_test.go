package proxy

import (
	"context"
	"crypto/sha256"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/pion/dtls/v3"
	"github.com/pion/dtls/v3/pkg/crypto/selfsign"
	"github.com/pion/ice/v4"
	"github.com/pion/sdp/v3"
	"github.com/pion/webrtc/v4"
)

// Data-channel-only NetherNet servers decline use_srtp in the DTLS handshake. The local
// dialer is the DTLS server, so it must keep the transport connected without an SRTP
// profile and continue into SCTP.
func TestLocalNetherNetDialAcceptsDataChannelOnlyServer(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()

	peers := make(chan error, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		offerSDP, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			peers <- err
			return
		}
		var session sdp.SessionDescription
		if err := session.Unmarshal(offerSDP); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			peers <- err
			return
		}
		offer, err := nethernet.ParseDescription(&session)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			peers <- err
			return
		}
		peer, err := startDataChannelOnlyPeer(ctx, offer)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			peers <- err
			return
		}
		w.Header().Set("Content-Type", "application/sdp")
		if _, err := w.Write(peer.answer); err != nil {
			peers <- err
			return
		}
		go func() { peers <- peer.exchange(ctx) }()
	}))
	t.Cleanup(server.Close)

	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("online"))
	target, err := resolve(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	dialed := make(chan error, 1)
	go func() {
		conn, err := target.network.DialContext(ctx, target.address)
		if err == nil {
			_ = conn.Close()
		}
		dialed <- err
	}()
	select {
	case err := <-peers:
		if err != nil {
			t.Fatalf("data-channel-only peer: %v", err)
		}
	case err := <-dialed:
		t.Fatalf("dial ended before the peer received SCTP: %v", err)
	case <-ctx.Done():
		t.Fatal("dial did not reach SCTP over a no-SRTP DTLS handshake")
	}
	cancel()
	<-dialed
}

// dataChannelOnlyPeer is a minimal NetherNet server that behaves like a vanilla
// data-channel-only peer: it answers with its own SDP and runs DTLS without use_srtp.
type dataChannelOnlyPeer struct {
	answer   []byte
	agent    *ice.Agent
	cert     tls.Certificate
	accepted chan acceptedICE
}

type acceptedICE struct {
	conn *ice.Conn
	err  error
}

// startDataChannelOnlyPeer gathers a loopback candidate and starts accepting the dialer
// as the ICE-controlled side. It returns the SDP answer to send back for the offer.
func startDataChannelOnlyPeer(ctx context.Context, offer *nethernet.Description) (*dataChannelOnlyPeer, error) {
	agent, err := ice.NewAgent(&ice.AgentConfig{
		NetworkTypes:    []ice.NetworkType{ice.NetworkTypeUDP4},
		IncludeLoopback: true,
		IPFilter:        func(ip net.IP) bool { return ip.IsLoopback() && ip.To4() != nil },
	})
	if err != nil {
		return nil, fmt.Errorf("create ICE agent: %w", err)
	}
	candidate, err := gatherLoopbackCandidate(ctx, agent)
	if err != nil {
		_ = agent.Close()
		return nil, err
	}
	ufrag, pwd, err := agent.GetLocalUserCredentials()
	if err != nil {
		_ = agent.Close()
		return nil, fmt.Errorf("obtain ICE credentials: %w", err)
	}
	cert, err := selfsign.GenerateSelfSigned()
	if err != nil {
		_ = agent.Close()
		return nil, fmt.Errorf("generate DTLS certificate: %w", err)
	}
	answer := nethernet.Description{
		ICE: webrtc.ICEParameters{UsernameFragment: ufrag, Password: pwd},
		DTLS: webrtc.DTLSParameters{
			Role:         webrtc.DTLSRoleClient,
			Fingerprints: []webrtc.DTLSFingerprint{{Algorithm: "sha-256", Value: dtlsFingerprint(cert)}},
		},
		SCTP: offer.SCTP,
		Candidates: []webrtc.ICECandidate{{
			Foundation: candidate.Foundation(),
			Priority:   candidate.Priority(),
			Address:    candidate.Address(),
			Protocol:   webrtc.ICEProtocolUDP,
			Port:       uint16(candidate.Port()),
			Component:  candidate.Component(),
			Typ:        webrtc.ICECandidateType(candidate.Type()),
		}},
	}
	identity, err := selfSignedIdentity(time.Now())
	if err != nil {
		_ = agent.Close()
		return nil, err
	}
	if answer.Identity, err = identity.Sign(&answer); err != nil {
		_ = agent.Close()
		return nil, fmt.Errorf("sign DTLS fingerprints: %w", err)
	}
	answerSDP, err := answer.Encode()
	if err != nil {
		_ = agent.Close()
		return nil, fmt.Errorf("encode SDP answer: %w", err)
	}
	peer := &dataChannelOnlyPeer{answer: answerSDP, agent: agent, cert: cert, accepted: make(chan acceptedICE, 1)}
	go func() {
		conn, err := agent.Accept(ctx, offer.ICE.UsernameFragment, offer.ICE.Password)
		peer.accepted <- acceptedICE{conn: conn, err: err}
	}()
	return peer, nil
}

// exchange runs the peer's DTLS client and waits for the dialer to start SCTP. A byte
// arriving after the handshake proves the dialer kept the transport connected without a
// negotiated SRTP profile.
func (p *dataChannelOnlyPeer) exchange(ctx context.Context) error {
	defer p.agent.Close()
	accepted := <-p.accepted
	if accepted.err != nil {
		return fmt.Errorf("accept ICE: %w", accepted.err)
	}
	defer accepted.conn.Close()
	conn, err := dtls.ClientWithOptions(packetConn{accepted.conn}, accepted.conn.RemoteAddr(),
		dtls.WithCertificates(p.cert), dtls.WithInsecureSkipVerify(true))
	if err != nil {
		return fmt.Errorf("start DTLS client: %w", err)
	}
	defer conn.Close()
	if err := conn.HandshakeContext(ctx); err != nil {
		return fmt.Errorf("DTLS handshake without use_srtp: %w", err)
	}
	deadline, _ := ctx.Deadline()
	_ = conn.SetReadDeadline(deadline)
	buf := make([]byte, 1500)
	if _, err := conn.Read(buf); err != nil {
		return fmt.Errorf("DTLS transport closed before SCTP: %w", err)
	}
	return nil
}

// gatherLoopbackCandidate waits for the first host candidate of a fresh agent.
func gatherLoopbackCandidate(ctx context.Context, agent *ice.Agent) (ice.Candidate, error) {
	var (
		mu       sync.Mutex
		first    ice.Candidate
		firstOne sync.Once
	)
	ready := make(chan struct{})
	if err := agent.OnCandidate(func(candidate ice.Candidate) {
		if candidate == nil {
			return
		}
		mu.Lock()
		first = candidate
		mu.Unlock()
		firstOne.Do(func() { close(ready) })
	}); err != nil {
		return nil, fmt.Errorf("watch ICE candidates: %w", err)
	}
	if err := agent.GatherCandidates(); err != nil {
		return nil, fmt.Errorf("gather ICE candidates: %w", err)
	}
	select {
	case <-ready:
	case <-ctx.Done():
		return nil, fmt.Errorf("gather ICE candidates: %w", ctx.Err())
	}
	mu.Lock()
	defer mu.Unlock()
	if first == nil {
		return nil, errors.New("no loopback ICE candidate")
	}
	return first, nil
}

// packetConn adapts an ICE stream to the packet interface DTLS expects.
type packetConn struct{ net.Conn }

func (c packetConn) ReadFrom(b []byte) (int, net.Addr, error) {
	n, err := c.Read(b)
	return n, c.RemoteAddr(), err
}

func (c packetConn) WriteTo(b []byte, _ net.Addr) (int, error) {
	return c.Write(b)
}

// dtlsFingerprint returns the SDP form of a certificate's SHA-256 fingerprint.
func dtlsFingerprint(cert tls.Certificate) string {
	sum := sha256.Sum256(cert.Certificate[0])
	parts := make([]string, len(sum))
	for i, b := range sum {
		parts[i] = fmt.Sprintf("%02X", b)
	}
	return strings.Join(parts, ":")
}
