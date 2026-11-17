package authcache

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

// A move waits for a refresh of either token, which holds that token's lease.
func TestMoveWaitsForBothTokensLeases(t *testing.T) {
	dir := t.TempDir()
	from, to := filepath.Join(dir, "pending.json"), filepath.Join(dir, "account.json")
	if err := os.WriteFile(from, []byte("new token"), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, held := range []string{from, to} {
		lease, err := lockfile.Acquire(held+cacheLockSuffix, 0)
		if err != nil {
			t.Fatal(err)
		}
		// The lease stays held, so the move can only give up when its context ends.
		waiting, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
		err = Move(waiting, from, to)
		cancel()
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("move passed the lease of %s", filepath.Base(held))
		}
		if _, err := os.Stat(from); err != nil {
			t.Fatalf("token moved past the lease of %s: %v", filepath.Base(held), err)
		}
		if err := lease.Close(); err != nil {
			t.Fatal(err)
		}
	}
	if err := Move(context.Background(), from, to); err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(to); err != nil || string(data) != "new token" {
		t.Fatalf("moved token = %q, %v", data, err)
	}
}
