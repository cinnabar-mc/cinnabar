package proxy

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
)

// Each Connect selects upstream caching without restarting the core or its local world.
func TestSessionCacheRequestControlsEachLoginOnAReusableCore(t *testing.T) {
	connections := newPreparedConnections("", nil, slog.New(slog.NewTextHandler(io.Discard, nil)))
	t.Cleanup(func() { _ = connections.shutdown() })
	connections.upstreamClientCache = true
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{offline: true}, nil
	}
	stopped := errors.New("fixture stops before dialing")
	for _, enabled := range []bool{true, false, true} {
		request := testSessionConnect(t)
		request.ClientCache = enabled
		downstream, err := newSessionDownstream(request, nil)
		if err != nil {
			t.Fatal(err)
		}
		called := false
		connections.dialTarget = func(_ context.Context, _ *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
			called = true
			if dialer.EnableClientCache != enabled {
				t.Errorf("upstream cache = %v, want connection request %v", dialer.EnableClientCache, enabled)
			}
			return nil, stopped
		}
		if _, err := connections.connect(context.Background(), downstream); !errors.Is(err, stopped) {
			t.Fatalf("connect error = %v, want the fixture dial boundary", err)
		}
		if !called {
			t.Fatal("the connection did not reach the upstream dialer")
		}
	}
}
