package proxy

import (
	"context"
	"errors"
	"log/slog"
	"reflect"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/service"
)

// testRelay counts relay config requests answered by answer, on a clock the test controls. Its
// methods are those of one join; shared reaches the account's relay for other joins.
type testRelay struct {
	*joinRelay
	shared   *xboxRelay
	requests atomic.Int32
	clock    time.Time
}

func newTestRelay(answer func(context.Context) (*nethernet.Credentials, error)) *testRelay {
	relay := &testRelay{clock: time.Unix(1_000_000, 0)}
	relay.shared = &xboxRelay{
		logger: slog.New(slog.DiscardHandler),
		now:    func() time.Time { return relay.clock },
		fetch: func(ctx context.Context) (*nethernet.Credentials, error) {
			relay.requests.Add(1)
			return answer(ctx)
		},
	}
	relay.joinRelay = relay.shared.join()
	return relay
}

// relayConfig is a signaling service answer shaped like the live one: one server listing a STUN
// and a TURN URL with TURN credentials, plus a TURN-only server.
func relayConfig() *nethernet.Credentials {
	return &nethernet.Credentials{ExpirationInSeconds: 60, ICEServers: []nethernet.ICEServer{
		{Username: "user", Password: "pass", URLs: []string{"stun:relay.example:3478", "turn:relay.example:3478"}},
		{Username: "user", Password: "pass", URLs: []string{"turns:relay.example:443"}},
	}}
}

// gatheringConfig is relayConfig as a connection gathers with it: its STUN URL alone.
func gatheringConfig() *nethernet.Credentials {
	return &nethernet.Credentials{ExpirationInSeconds: 60, ICEServers: []nethernet.ICEServer{
		{URLs: []string{"stun:relay.example:3478"}},
	}}
}

// A signed-in addressed NetherNet join gathers through the account's relay config, requested
// once and copied per connection.
func TestSignedInAddressedJoinGathersWithAccountRelay(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) { return relayConfig(), nil })
	gather := remoteServerNetwork(slog.New(slog.DiscardHandler), nil, relay.joinRelay).NetherNet.Dialer.Credentials
	if gather == nil {
		t.Fatal("signed-in addressed joins gather local candidates only")
	}
	first, err := gather(t.Context())
	if err != nil || !reflect.DeepEqual(first, gatheringConfig()) {
		t.Fatalf("gathering config = %+v, %v; want the account relay config's STUN URL", first, err)
	}
	first.ICEServers[0].URLs[0] = "stun:changed.invalid:3478"
	second, err := gather(t.Context())
	if err != nil || !reflect.DeepEqual(second, gatheringConfig()) {
		t.Fatal("one connection changed another connection's relay config")
	}
	if got := relay.requests.Load(); got != 1 {
		t.Fatalf("relay config requested %d times, want 1", got)
	}
}

// Signed-out joins have no account relay config and gather local candidates only.
func TestSignedOutAddressedJoinGathersLocally(t *testing.T) {
	if remoteServerNetwork(slog.New(slog.DiscardHandler), nil, nil).NetherNet.Dialer.Credentials != nil {
		t.Fatal("signed-out addressed joins request a relay config")
	}
}

// A refused relay config request leaves the join gathering local candidates, and the next
// connection asks again.
func TestUnavailableRelayConfigGathersLocally(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		return nil, errors.New("signaling unavailable")
	})
	for range 2 {
		credentials, err := relay.Credentials(t.Context())
		if err != nil || credentials != nil {
			t.Fatalf("gathering config = %+v, %v; want local candidates only", credentials, err)
		}
	}
	if got := relay.requests.Load(); got != 2 {
		t.Fatalf("relay config requested %d times, want 2", got)
	}
}

// A join that prefetched a failed relay config gathers locally without requesting it again; the
// next connection asks again.
func TestFailedRelayPrefetchServesItsJoin(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		return nil, errors.New("signaling unavailable")
	})
	<-relay.start(t.Context()).done
	for want := int32(1); want <= 2; want++ {
		if credentials, err := relay.Credentials(t.Context()); err != nil || credentials != nil {
			t.Fatalf("gathering config = %+v, %v; want local candidates only", credentials, err)
		}
		if got := relay.requests.Load(); got != want {
			t.Fatalf("relay config requested %d times, want %d", got, want)
		}
	}
}

// A request that ignores its deadline still releases the join to local candidates at the bound.
func TestStalledRelayRequestReleasesJoinAtItsBound(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		release := make(chan struct{})
		relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
			<-release
			return relayConfig(), nil
		})
		started := time.Now()
		credentials, err := relay.Credentials(t.Context())
		waited := time.Since(started)
		close(release)
		if err != nil || credentials != nil {
			t.Fatalf("gathering config = %+v, %v; want local candidates only", credentials, err)
		}
		if waited != relayFetchTimeout {
			t.Fatalf("join waited %s, want %s", waited, relayFetchTimeout)
		}
		synctest.Wait()
	})
}

// A relay config is reused until the signaling service's expiry, then requested again.
func TestExpiredRelayConfigIsRequestedAgain(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) { return relayConfig(), nil })
	for _, advance := range []time.Duration{0, 59 * time.Second, 2 * time.Second} {
		relay.clock = relay.clock.Add(advance)
		if _, err := relay.Credentials(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	if got := relay.requests.Load(); got != 2 {
		t.Fatalf("relay config requested %d times, want 2", got)
	}
}

// A transfer to another addressed server keeps the target's relay config.
func TestAddressedTransferKeepsTargetRelay(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) { return relayConfig(), nil })
	target := &resolvedUpstreamTarget{address: "first.example:19132", relay: relay.joinRelay}
	if _, err := relay.Credentials(t.Context()); err != nil {
		t.Fatal(err)
	}
	transfer, ok := networkForAddress(target, "second.example:19132", nil).(addressedServerNetwork)
	if !ok || transfer.NetherNet.Dialer.Credentials == nil {
		t.Fatal("transfer lost the signed-in relay config")
	}
	if credentials, err := transfer.NetherNet.Dialer.Credentials(t.Context()); err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("transfer gathering config = %+v, %v", credentials, err)
	}
	if got := relay.requests.Load(); got != 1 {
		t.Fatalf("relay config requested %d times, want 1", got)
	}
}

// A join that gives up stops waiting, while the request it started still completes for the
// next connection.
func TestRelayRequestOutlivesCancelledWait(t *testing.T) {
	release := make(chan struct{})
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		<-release
		return relayConfig(), nil
	})
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := relay.Credentials(cancelled); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled wait = %v, want context.Canceled", err)
	}
	close(release)
	if credentials, err := relay.Credentials(t.Context()); err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("gathering config = %+v, %v", credentials, err)
	}
	if got := relay.requests.Load(); got != 1 {
		t.Fatalf("relay config requested %d times, want 1", got)
	}
}

// Selecting NetherNet starts the relay config request, which the HTTP join then reuses.
func TestAddressedHTTPJoinPrefetchesRelayConfig(t *testing.T) {
	address := trustedListenerAddress(t)
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		return &nethernet.Credentials{ExpirationInSeconds: 60}, nil
	})
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	connection, err := remoteServerNetwork(slog.New(slog.DiscardHandler), nil, relay.joinRelay).DialContext(ctx, address)
	if err != nil {
		t.Fatal(err)
	}
	_ = connection.Close()
	if got := relay.requests.Load(); got != 1 {
		t.Fatalf("relay config requested %d times, want 1", got)
	}
}

// Connections gather through the relay config's STUN URL only. Its TURN server cannot allocate for
// pion, so a TURN URL only delayed the offer by about 8 s, and it would relay the join through
// Microsoft's servers, which servers' VPN checks flag.
func TestRelayGathersWithoutTURN(t *testing.T) {
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) { return relayConfig(), nil })
	credentials, err := relay.Credentials(t.Context())
	if err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("gathering config = %+v, %v; want STUN only", credentials, err)
	}
	turnOnly := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		return &nethernet.Credentials{ExpirationInSeconds: 60, ICEServers: relayConfig().ICEServers[1:]}, nil
	})
	if credentials, err := turnOnly.Credentials(t.Context()); err != nil || len(credentials.ICEServers) != 0 {
		t.Fatalf("TURN-only gathering config = %+v, %v; want no ICE servers", credentials, err)
	}
}

// Joins with the same account share one relay config, so only the first join waits for it;
// another account gets its own.
func TestRelayConfigIsSharedAcrossJoins(t *testing.T) {
	logger := slog.New(slog.DiscardHandler)
	first, second := &fakeTokenSource{name: "first"}, &fakeTokenSource{name: "second"}
	if relayForAccount(first, logger) != relayForAccount(first, logger) {
		t.Fatal("two joins with one account requested separate relay configs")
	}
	if relayForAccount(first, logger) == relayForAccount(second, logger) {
		t.Fatal("another account reused the previous account's relay config")
	}
}

// fakeTokenSource is a distinct account that is never asked for a token.
type fakeTokenSource struct{ name string }

func (*fakeTokenSource) ServiceToken(context.Context) (*service.Token, error) {
	return nil, errors.New("fake account issues no tokens")
}

// Realm, friend and raw NetherNet joins gather through the STUN URLs of the signaling service's
// ICE servers only, as addressed joins do.
func TestSignaledJoinGathersWithoutTURN(t *testing.T) {
	signaling := stunOnlySignaling{credentialsSignaling{credentials: relayConfig()}}
	credentials, err := signaling.Credentials(t.Context())
	if err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("gathering config = %+v, %v; want STUN only", credentials, err)
	}
	none, err := stunOnlySignaling{credentialsSignaling{}}.Credentials(t.Context())
	if err != nil || none != nil {
		t.Fatalf("gathering config without ICE servers = %+v, %v; want nil", none, err)
	}
}

// credentialsSignaling is signaling that only answers Credentials.
type credentialsSignaling struct {
	minecraft.SignalingConn
	credentials *nethernet.Credentials
}

func (s credentialsSignaling) Credentials(context.Context) (*nethernet.Credentials, error) {
	return s.credentials, nil
}

// A join that prefetched a failed relay config and never asked for it leaves no failure behind:
// the next join requests again. A config that arrived is shared with the next join.
func TestRelayFailureStaysWithItsJoin(t *testing.T) {
	failing := true
	relay := newTestRelay(func(context.Context) (*nethernet.Credentials, error) {
		if failing {
			return nil, errors.New("signaling unavailable")
		}
		return relayConfig(), nil
	})
	<-relay.start(t.Context()).done
	failing = false
	next := relay.shared.join()
	if credentials, err := next.Credentials(t.Context()); err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("next join gathering config = %+v, %v; want a fresh request's STUN URL", credentials, err)
	}
	if credentials, err := relay.shared.join().Credentials(t.Context()); err != nil || !reflect.DeepEqual(credentials, gatheringConfig()) {
		t.Fatalf("third join gathering config = %+v, %v", credentials, err)
	}
	if got := relay.requests.Load(); got != 2 {
		t.Fatalf("relay config requested %d times, want 2", got)
	}
}
