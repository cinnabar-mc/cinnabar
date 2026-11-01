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

// relayFetch is one relay config request; credentials is nil when it failed. A failure is handed
// to one connection, so a join waiting on its own prefetch does not request again.
type relayFetch struct {
	done        chan struct{}
	credentials *nethernet.Credentials
	expires     time.Time
	consumed    bool // a connection took this failure; guarded by xboxRelay.mu
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
			// The graceful close has its own timeouts; it must not hold back the answer.
			defer func() { go func() { _ = conn.Close() }() }()
			return conn.Credentials(ctx)
		},
	}
}

// start returns the running request, an unexpired config, or a failure no connection has taken,
// and otherwise begins a request. The request outlives ctx's cancellation so a join that selected
// NetherNet early can prefetch, and it completes by relayFetchTimeout even if the fetch does not.
func (r *xboxRelay) start(ctx context.Context) *relayFetch {
	r.mu.Lock()
	defer r.mu.Unlock()
	if current := r.pending; current != nil {
		select {
		case <-current.done:
			if current.credentials == nil && !current.consumed ||
				current.credentials != nil && r.now().Before(current.expires) {
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
		type answer struct {
			credentials *nethernet.Credentials
			err         error
		}
		answered := make(chan answer, 1)
		go func() {
			credentials, err := r.fetch(fetchContext)
			answered <- answer{credentials: credentials, err: err}
		}()
		var result answer
		select {
		case result = <-answered:
		case <-fetchContext.Done():
			result.err = context.Cause(fetchContext)
		}
		if result.err != nil || result.credentials == nil {
			r.logger.Warn("NetherNet relay config unavailable; gathering local candidates only", "error", redact.Error(result.err))
			return
		}
		fetch.credentials = result.credentials
		fetch.expires = r.now().Add(time.Duration(result.credentials.ExpirationInSeconds) * time.Second)
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
		r.mu.Lock()
		fetch.consumed = true
		r.mu.Unlock()
		return nil, nil
	}
	credentials := *fetch.credentials
	credentials.ICEServers = slices.Clone(credentials.ICEServers)
	for i := range credentials.ICEServers {
		credentials.ICEServers[i].URLs = slices.Clone(credentials.ICEServers[i].URLs)
	}
	return &credentials, nil
}
