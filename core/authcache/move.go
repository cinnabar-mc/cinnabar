package authcache

import (
	"context"
	"errors"
	"os"
	"sort"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

// Move renames the token cache at from to to while holding both caches' leases, so a refresh
// of either in this or another process cannot race the move. Leases are taken in path order,
// so two moves between the same caches cannot deadlock.
func Move(ctx context.Context, from, to string) (err error) {
	paths := []string{from, to}
	sort.Strings(paths)
	for _, path := range paths {
		lease, err := lockfile.AcquireContext(ctx, path+cacheLockSuffix)
		if err != nil {
			return err
		}
		defer func() { err = errors.Join(err, lease.Close()) }()
	}
	return os.Rename(from, to)
}
