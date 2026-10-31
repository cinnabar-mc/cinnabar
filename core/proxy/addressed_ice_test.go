package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net/url"
	"os"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
)

// Addressed HTTP peers must publish a usable public candidate behind NAT.
func TestAddressedHTTPProvidesPublicICECredentials(t *testing.T) {
	network := remoteServerNetwork(slog.New(slog.DiscardHandler), nil)
	credentials := network.NetherNet.Dialer.Credentials
	if credentials == nil {
		t.Fatal("addressed HTTP connections have no public ICE gathering provider")
	}
	first, err := credentials(t.Context())
	if err != nil || first == nil || len(first.ICEServers) == 0 {
		t.Fatalf("ICE credentials = %v, %v", first, err)
	}
	for _, server := range first.ICEServers {
		if len(server.URLs) == 0 || server.Username != "" || server.Password != "" {
			t.Fatal("public STUN gathering requires URLs and no account credentials")
		}
		for _, raw := range server.URLs {
			parsed, err := url.Parse(raw)
			if err != nil || parsed.Scheme != "stun" || parsed.Opaque == "" {
				t.Fatalf("invalid public STUN service %q", raw)
			}
		}
	}
	original := first.ICEServers[0].URLs[0]
	first.ICEServers[0].URLs[0] = "stun:changed.invalid:3478"
	second, err := credentials(t.Context())
	if err != nil || second.ICEServers[0].URLs[0] != original {
		t.Fatal("one connection changed another connection's gathering credentials")
	}
}

// Both signed-in and signed-out loopback joins work without a public service.
func TestAddressedHTTPLoopbackDoesNotRequestPublicCredentials(t *testing.T) {
	for _, signedIn := range []bool{false, true} {
		t.Run(map[bool]string{false: "signed_out", true: "signed_in"}[signedIn], func(t *testing.T) {
			address := trustedListenerAddress(t)
			network := remoteServerNetwork(slog.New(slog.DiscardHandler), nil)
			var calls atomic.Int32
			network.NetherNet.Dialer.Credentials = func(context.Context) (*nethernet.Credentials, error) {
				calls.Add(1)
				return nil, errors.New("public gathering must not run for loopback")
			}
			ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
			defer cancel()
			if signedIn {
				identity, err := selfSignedIdentity(time.Now())
				if err != nil {
					t.Fatal(err)
				}
				connection, err := network.DialContextIdentityProvider(ctx, address, identity.Token, identity.PrivateKey, identity.Domain)
				if err != nil {
					t.Fatal(err)
				}
				_ = connection.Close()
			} else {
				connection, err := network.DialContext(ctx, address)
				if err != nil {
					t.Fatal(err)
				}
				_ = connection.Close()
			}
			if calls.Load() != 0 || network.NetherNet.Dialer.Credentials == nil {
				t.Fatal("loopback gathering used a public service or changed later joins")
			}
		})
	}
}

// An explicit endpoint fixture exercises the actual addressed join factory.
func TestAddressedHTTPPublicTraversal(t *testing.T) {
	address := os.Getenv("CINNABAR_TEST_NETHERNET_ENDPOINT")
	if address == "" {
		t.Skip("missing CINNABAR_TEST_NETHERNET_ENDPOINT fixture")
	}
	if strings.Contains(address, "/") {
		t.Fatal("fixture must be a host:port address")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	network := remoteServerNetwork(slog.New(slog.DiscardHandler), nil)
	connection, err := network.DialContext(ctx, address)
	if err != nil {
		t.Fatalf("addressed HTTP transport: %v", err)
	}
	if err := connection.Close(); err != nil {
		t.Fatal(err)
	}
}
