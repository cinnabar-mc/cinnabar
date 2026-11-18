package proxy

import (
	"bytes"
	"context"
	"fmt"
	"log/slog"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/pion/datachannel"
	"github.com/pion/dtls/v3"
	"github.com/pion/dtls/v3/pkg/crypto/selfsign"
	"github.com/pion/ice/v4"
	"github.com/pion/logging"
	"github.com/pion/sctp"
	"github.com/pion/sdp/v3"
)

// A server may send one SCTP message as large as vanilla's receive window. Each production
// NetherNet dialer must deliver it: a receiver whose buffer is smaller than the message
// advertises a zero window before the message completes and drops the rest of it, so the join
// hangs with the association still open.
func TestNetherNetDialersReceiveMessagesUpToTheVanillaWindow(t *testing.T) {
	logger := slog.New(slog.DiscardHandler)
	dialers := map[string]nethernet.Dialer{
		"addressed server":   remoteServerNetwork(logger, nil, nil).NetherNet.Dialer,
		"HTTP signaling":     httpNetherNetNetwork(logger, nil, nil).NetherNet.Dialer,
		"JSON-RPC signaling": scopedNetherNetNetwork{logger: logger}.transport().Dialer,
		"local server":       {API: localNetherNetAPI(), AllowIdentitylessServer: true},
	}
	// One unsegmented message, well over pion's 1 MiB default and inside vanilla's window.
	message := bytes.Repeat([]byte{0x5a}, vanillaSCTPReceiveWindow-(1<<20))
	for name, dialer := range dialers {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
			defer cancel()
			signaling := newOneServerSignaling(ctx)
			served := make(chan error, 1)
			go func() { served <- serveOneMessage(ctx, signaling, message) }()

			conn, err := dialer.DialContext(ctx, oneServerNetworkID, signaling)
			if err != nil {
				t.Fatalf("dial: %v", err)
			}
			defer conn.Close()
			if _, err := conn.Write([]byte("request network settings")); err != nil {
				t.Fatalf("write: %v", err)
			}
			_ = conn.SetReadDeadline(time.Now().Add(10 * time.Second))
			got, err := conn.ReadPacket()
			if err != nil {
				select {
				case serveErr := <-served:
					t.Fatalf("read the %d-byte message: %v (server: %v)", len(message), err, serveErr)
				default:
					t.Fatalf("read the %d-byte message: %v", len(message), err)
				}
			}
			if !bytes.Equal(got, message) {
				t.Fatalf("received %d bytes, want the %d-byte message", len(got), len(message))
			}
		})
	}
}

// serveOneMessage answers one offer like a data-channel-only NetherNet server, waits for the
// dialer's first packet, then sends message as a single SCTP message with a zero segment count.
func serveOneMessage(ctx context.Context, signaling *oneServerSignaling, message []byte) error {
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
	association, err := sctp.Client(sctp.Config{
		NetConn: dtlsConn, LoggerFactory: loggers, MaxMessageSize: uint32(len(message) + 1),
	})
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
		if _, err := channel.Read(make([]byte, 1024)); err != nil {
			return fmt.Errorf("read reliable channel: %w", err)
		}
		if _, err := channel.Write(append([]byte{0}, message...)); err != nil {
			return fmt.Errorf("write message: %w", err)
		}
		// Keep the association open until the dialer has read the message.
		<-ctx.Done()
		return nil
	}
}
