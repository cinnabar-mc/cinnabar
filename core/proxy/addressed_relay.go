package proxy

import (
	"context"
	"log/slog"
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
// can reach each other. One xboxRelay is shared by the account's joins, which reach it through
// their own joinRelay.
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
			// The graceful close has its own timeouts; it must not hold back the answer.
			defer func() { go func() { _ = conn.Close() }() }()
			return conn.Credentials(ctx)
		},
	}
}

// start returns the running request or an unexpired config, and otherwise begins a request; a
// failed request is never returned again, so every later join asks anew. The request outlives
// ctx's cancellation so a join that selected NetherNet early can prefetch, and it completes by
// relayFetchTimeout even if the fetch does not.
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

// join returns a handle for one join and the transfers that follow it.
func (r *xboxRelay) join() *joinRelay { return &joinRelay{shared: r} }

// joinRelay is one join's view of its account's shared relay config. It remembers the request the
// join prefetched, so the join's first connection takes that request's answer, failed or not,
// instead of requesting again; a failure never reaches another join or a later connection.
type joinRelay struct {
	shared *xboxRelay

	mu         sync.Mutex
	prefetched *relayFetch
}

// start prefetches the relay config for this join's next connection.
func (j *joinRelay) start(ctx context.Context) *relayFetch {
	fetch := j.shared.start(ctx)
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.prefetched == nil {
		j.prefetched = fetch
	}
	return fetch
}

// Credentials returns the relay config's STUN servers for one connection's candidate gathering,
// or nil when none could be obtained, which gathers local candidates only.
func (j *joinRelay) Credentials(ctx context.Context) (*nethernet.Credentials, error) {
	j.mu.Lock()
	fetch := j.prefetched
	j.prefetched = nil
	j.mu.Unlock()
	if fetch == nil {
		fetch = j.shared.start(ctx)
	}
	select {
	case <-fetch.done:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	if fetch.credentials == nil {
		return nil, nil
	}
	return stunOnly(fetch.credentials), nil
}

// accountRelays keeps the most recent signed-in account's relay config across joins, as vanilla
// holds its config from sign-in, so only the first NetherNet join after starting or switching
// accounts waits for the request. The config lasts as long as the service says, a week today.
var accountRelays struct {
	mu      sync.Mutex
	account service.TokenSource
	relay   *xboxRelay
}

// relayForAccount returns account's shared relay config, replacing the previous account's.
func relayForAccount(account service.TokenSource, logger *slog.Logger) *xboxRelay {
	accountRelays.mu.Lock()
	defer accountRelays.mu.Unlock()
	if accountRelays.relay == nil || accountRelays.account != account {
		accountRelays.account, accountRelays.relay = account, newXboxRelay(account, logger)
	}
	return accountRelays.relay
}
