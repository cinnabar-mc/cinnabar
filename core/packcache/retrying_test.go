package packcache

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
)

// retryingEvent records one availability change reported by a Retrying cache.
type retryingEvent struct {
	available bool
	err       error
}

func openerFor(root string) func() (Opened, error) {
	return func() (Opened, error) { return New(root, WithQuota(1<<20)) }
}

// TestRetryingRecoversOnceThePreviousHolderReleasesTheCache reproduces a client that starts
// while the previous client's core still holds the cache lease: the packs the previous
// session downloaded must be served as soon as the lease frees, not only after a restart.
func TestRetryingRecoversOnceThePreviousHolderReleasesTheCache(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	holder, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatal(err)
	}
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "earlier session")
	if err := holder.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}

	var events []retryingEvent
	cache := NewRetrying(openerFor(root), func(available bool, err error) {
		events = append(events, retryingEvent{available, err})
	})
	t.Cleanup(func() { _ = cache.Close() })
	if len(events) != 1 || events[0].available || !errors.Is(events[0].err, ErrInUse) {
		t.Fatalf("events after a blocked open = %+v, want one unavailable report caused by ErrInUse", events)
	}

	if got, err := cache.Load(context.Background(), key); !errors.Is(err, ErrUnavailable) || got != nil {
		t.Fatalf("Load while the lease is held = %v, %v; want ErrUnavailable", got, err)
	}
	if err := cache.Store(context.Background(), key, pack); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("Store while the lease is held = %v; want ErrUnavailable", err)
	}
	if _, _, _, err := cache.Reference(pack); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("Reference while the lease is held = %v; want ErrUnavailable", err)
	}
	if len(events) != 1 {
		t.Fatalf("repeated failures reported again: %+v", events)
	}

	if err := holder.Close(); err != nil {
		t.Fatal(err)
	}
	got, err := cache.Load(context.Background(), key)
	if err != nil || got == nil {
		t.Fatalf("Load after the lease was released = %v, %v; want the earlier session's pack", got, err)
	}
	if len(events) != 2 || !events[1].available || events[1].err != nil {
		t.Fatalf("events after recovery = %+v, want an available report", events)
	}
	if _, err := cache.Load(context.Background(), key); err != nil || len(events) != 2 {
		t.Fatalf("later Load = %v with events %+v; want no further reports", err, events)
	}
}

func TestRetryingOpensImmediatelyWhenTheCacheIsFree(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	reported := false
	cache := NewRetrying(openerFor(root), func(bool, error) { reported = true })
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "free")
	if err := cache.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	path, checksum, release, err := cache.Reference(pack)
	if err != nil {
		t.Fatalf("Reference = %v", err)
	}
	release()
	if checksum != pack.Checksum() {
		t.Fatal("Reference returned another archive's checksum")
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("lent archive missing: %v", err)
	}
	if reported {
		t.Fatal("an open that never failed reported a change")
	}
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatalf("Close did not release the lease: %v", err)
	}
	defer reopened.Close()
	if got, err := reopened.Load(context.Background(), key); err != nil || got == nil {
		t.Fatalf("stored pack lost: %v, %v", got, err)
	}
}

func TestRetryingCloseStopsRetriesAndIsIdempotent(t *testing.T) {
	root := filepath.Join(secureTempDir(t), "objects")
	holder, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatal(err)
	}
	defer holder.Close()
	cache := NewRetrying(openerFor(root), nil)
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	if err := holder.Close(); err != nil {
		t.Fatal(err)
	}
	_, key, _ := testPack(t, uuid.New(), "1.0.0", "closed")
	if _, err := cache.Load(context.Background(), key); !errors.Is(err, ErrClosed) {
		t.Fatalf("Load after Close = %v; a closed cache must not reopen the root", err)
	}
	reopened, err := New(root, WithQuota(1<<20))
	if err != nil {
		t.Fatalf("closed Retrying took the lease: %v", err)
	}
	_ = reopened.Close()
}
