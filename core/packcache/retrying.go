package packcache

import (
	"context"
	"errors"
	"fmt"
	"io"
	"sync"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// ErrUnavailable reports that the cache root could not be opened. The next
// operation on a Retrying cache tries again.
var ErrUnavailable = errors.New("packcache: cache unavailable")

// Opened is a process-owned cache that must be closed.
type Opened interface {
	minecraft.ResourcePackCache
	io.Closer
}

// archiveLender is implemented by caches that can lend an on-disk archive.
type archiveLender interface {
	Reference(*resource.Pack) (path string, checksum [32]byte, release func(), err error)
}

// Retrying is a minecraft.ResourcePackCache that keeps trying to open its
// cache. The cache root is leased to one process at a time, and a previous
// client's core can outlive its client for a while, so a failed open must not
// disable the cache for the rest of the process. While the cache is closed to
// us, Load and Store report ErrUnavailable and downloads proceed uncached.
type Retrying struct {
	open    func() (Opened, error)
	changed func(available bool, err error)

	mu      sync.Mutex
	cache   Opened
	failing bool
	closed  bool
}

// NewRetrying opens the cache once and returns a cache that retries on later use.
// changed, which may be nil, runs when opening first fails (available is false, with
// the cause) and when a later attempt succeeds (available is true). It never runs
// for an open that succeeds without a preceding failure. It must not call the cache.
func NewRetrying(open func() (Opened, error), changed func(available bool, err error)) *Retrying {
	r := &Retrying{open: open, changed: changed}
	_, _ = r.acquire()
	return r
}

// acquire returns the open cache, opening it when it is not open yet.
func (r *Retrying) acquire() (Opened, error) {
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		return nil, ErrClosed
	}
	if r.cache != nil {
		cache := r.cache
		r.mu.Unlock()
		return cache, nil
	}
	cache, err := r.open()
	if err != nil {
		first := !r.failing
		r.failing = true
		r.mu.Unlock()
		if first && r.changed != nil {
			r.changed(false, err)
		}
		return nil, fmt.Errorf("%w: %w", ErrUnavailable, err)
	}
	r.cache = cache
	recovered := r.failing
	r.failing = false
	r.mu.Unlock()
	if recovered && r.changed != nil {
		r.changed(true, nil)
	}
	return cache, nil
}

// Load returns the cached pack, a miss, or ErrUnavailable while the cache cannot be opened.
func (r *Retrying) Load(ctx context.Context, key minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	cache, err := r.acquire()
	if err != nil {
		return nil, err
	}
	return cache.Load(ctx, key)
}

// Store persists pack, or returns ErrUnavailable while the cache cannot be opened.
func (r *Retrying) Store(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack) error {
	cache, err := r.acquire()
	if err != nil {
		return err
	}
	return cache.Store(ctx, key, pack)
}

// Reference lends pack's cached archive. It fails while the cache cannot be opened or
// cannot lend archives, and the caller then streams the archive instead.
func (r *Retrying) Reference(pack *resource.Pack) (string, [32]byte, func(), error) {
	cache, err := r.acquire()
	if err != nil {
		return "", [32]byte{}, nil, err
	}
	lender, ok := cache.(archiveLender)
	if !ok {
		return "", [32]byte{}, nil, errors.New("packcache: cache cannot lend archives")
	}
	return lender.Reference(pack)
}

// Close closes the cache if it is open. Later operations fail with ErrClosed. Close is idempotent.
func (r *Retrying) Close() error {
	r.mu.Lock()
	r.closed = true
	cache := r.cache
	r.cache = nil
	r.mu.Unlock()
	if cache == nil {
		return nil
	}
	return cache.Close()
}

var _ Opened = (*Retrying)(nil)
