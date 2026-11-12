package packcache

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Installed serves resource packs the player already installed before falling back to another cache.
// The vanilla client checks its pack repository for each offered UUID and version before downloading.
// Installed matches the same identity, plus the advertised size that gophertunnel requires of every
// cache hit. It never writes to the installed directory: downloads are stored in the fallback cache.
type Installed struct {
	dir  string
	next minecraft.ResourcePackCache
}

// archiveReferencer is the session handoff's view of a cache that can lend archive paths.
type archiveReferencer interface {
	Reference(pack *resource.Pack) (path string, checksum [32]byte, release func(), err error)
}

// NewInstalled layers the installed archives in dir over next, which may be nil.
// The directory belongs to the client's pack library and may be missing or change at any time.
func NewInstalled(dir string, next minecraft.ResourcePackCache) *Installed {
	return &Installed{dir: dir, next: next}
}

// Load returns an installed archive with the key's UUID, version and size, or else defers to the
// fallback cache. Unreadable or malformed installed archives are skipped.
func (c *Installed) Load(ctx context.Context, key minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	if pack := c.loadInstalled(ctx, key); pack != nil {
		return pack, nil
	}
	if c.next == nil {
		return nil, nil
	}
	return c.next.Load(ctx, key)
}

// loadInstalled scans the directory once per offered pack. Only regular archives whose size equals the
// key's are parsed, so a miss costs one directory listing.
func (c *Installed) loadInstalled(ctx context.Context, key minecraft.ResourcePackCacheKey) *resource.Pack {
	if c.dir == "" {
		return nil
	}
	entries, err := os.ReadDir(c.dir)
	if err != nil {
		return nil
	}
	for _, entry := range entries {
		if ctx.Err() != nil {
			return nil
		}
		if !strings.EqualFold(filepath.Ext(entry.Name()), objectSuffix) {
			continue
		}
		info, err := entry.Info()
		if err != nil || !regularNoLink(info) || info.Size() < 0 || uint64(info.Size()) != key.Size {
			continue
		}
		pack, err := resource.ReadPath(filepath.Join(c.dir, entry.Name()))
		if err == nil && key.Matches(pack) {
			return pack
		}
	}
	return nil
}

// Store saves downloaded packs in the fallback cache only.
func (c *Installed) Store(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack) error {
	if c.next == nil {
		return nil
	}
	return c.next.Store(ctx, key, pack)
}

// Reference lends an archive held by the fallback cache. Installed archives are not lent, so the session
// streams them instead of depending on files the pack library may replace.
func (c *Installed) Reference(pack *resource.Pack) (string, [32]byte, func(), error) {
	if next, ok := c.next.(archiveReferencer); ok {
		return next.Reference(pack)
	}
	return "", [32]byte{}, nil, errors.New("packcache: no archive cache to reference")
}
