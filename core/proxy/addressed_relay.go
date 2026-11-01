package proxy

import (
	"context"
	"log/slog"
	"slices"
	"sync"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/signaling/messaging"
)

// relayFetchTimeout bounds one relay config request. When it passes, the join continues with
// local candidates only, as it does when the signaling service refuses the request.
const relayFetchTimeout = 10 * time.Second

// xboxRelay supplies a signed-in account's NetherNet relay config: the STUN and TURN servers the
// Player Messaging signaling service issues. Vanilla gathers candidates for every NetherNet
// connection with this config, including addressed joins signaled over HTTP, so peers behind NAT
// can reach each other. One xboxRelay serves a join and the transfers that follow it.
type xboxRelay struct {
	fetch  func(context.Context) (*nethernet.Credentials, error)
	logger *slog.Logger
	now    func() time.Time

	mu      sync.Mutex
	pending *relayFetch
}

// relayFetch is one relay config request; credentials is nil when it failed.
type relayFetch struct {
	done        chan struct{}
	credentials *nethernet.Credentials
	expires     time.Time
}

// newXboxRelay requests account's relay config over a short-lived Player Messaging connection.
func newXboxRelay(account service.TokenSource, logger *slog.Logger) *xboxRelay {
	return &xboxRelay{
		logger: logger,
		now:    time.Now,
		fetch: func(ctx context.Context) (*nethernet.Credentials, error) {
			conn, err := messaging.Dialer{Log: logger}.DialContext(ctx, account)
			if err != nil {
				return nil, err
			}
			defer func() { _ = conn.Close() }()
			return conn.Credentials(ctx)
		},
	}
}

// start begins a request unless one is running or an unexpired config is held, and returns it.
// The request outlives ctx's cancellation so a join that selected NetherNet early can prefetch.
func (r *xboxRelay) start(ctx context.Context) *relayFetch {
	r.mu.Lock()
	defer r.mu.Unlock()
	if current := r.pending; current != nil {
		select {
		case <-current.done:
			if current.credentials != nil && r.now().Before(current.expires) {
				return current
			}
		default:
			return current
		}
	}
	fetch := &relayFetch{done: make(chan struct{})}
	r.pending = fetch
	go func() {
		defer close(fetch.done)
		fetchContext, cancel := context.WithTimeout(context.WithoutCancel(ctx), relayFetchTimeout)
		defer cancel()
		credentials, err := r.fetch(fetchContext)
		if err != nil || credentials == nil {
			r.logger.Warn("NetherNet relay config unavailable; gathering local candidates only", "error", redact.Error(err))
			return
		}
		fetch.credentials = credentials
		fetch.expires = r.now().Add(time.Duration(credentials.ExpirationInSeconds) * time.Second)
	}()
	return fetch
}

// Credentials returns a copy of the relay config for one connection's candidate gathering, or
// nil when none could be obtained, which gathers local candidates only.
func (r *xboxRelay) Credentials(ctx context.Context) (*nethernet.Credentials, error) {
	fetch := r.start(ctx)
	select {
	case <-fetch.done:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	if fetch.credentials == nil {
		return nil, nil
	}
	credentials := *fetch.credentials
	credentials.ICEServers = slices.Clone(credentials.ICEServers)
	for i := range credentials.ICEServers {
		credentials.ICEServers[i].URLs = slices.Clone(credentials.ICEServers[i].URLs)
	}
	return &credentials, nil
}
