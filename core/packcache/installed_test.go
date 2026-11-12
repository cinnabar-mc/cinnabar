package packcache

import (
	"archive/zip"
	"bytes"
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// storedPack builds an uncompressed archive, so equal-length versions and payloads give equal sizes.
func storedPack(t *testing.T, id uuid.UUID, version, payload string) (*resource.Pack, minecraft.ResourcePackCacheKey, []byte) {
	t.Helper()
	var b bytes.Buffer
	zw := zip.NewWriter(&b)
	files := map[string]string{
		"manifest.json": fmt.Sprintf(`{"format_version":2,"header":{"name":"test","description":"test","uuid":"%s","version":[%s],"min_engine_version":[1,0,0]},"modules":[{"type":"resources","uuid":"%s","version":[%s]}]}`, id, csvVersion(version), uuid.New(), csvVersion(version)),
		"payload.txt":   payload,
	}
	for _, name := range []string{"manifest.json", "payload.txt"} {
		w, err := zw.CreateHeader(&zip.FileHeader{Name: name, Method: zip.Store})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Write([]byte(files[name])); err != nil {
			t.Fatal(err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	archive := b.Bytes()
	pack, err := resource.ReadBytes(archive)
	if err != nil {
		t.Fatal(err)
	}
	return pack, minecraft.ResourcePackCacheKey{UUID: id, Version: version, Size: uint64(len(archive))}, archive
}

// writeInstalled places an archive in an installed-pack directory under an arbitrary name.
func writeInstalled(t *testing.T, dir, name string, archive []byte) {
	t.Helper()
	if err := os.WriteFile(filepath.Join(dir, name), archive, 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestInstalledServesMatchingIdentityBeforeFallback(t *testing.T) {
	dir := t.TempDir()
	id := uuid.New()
	installedPack, key, archive := storedPack(t, id, "1.2.3", "installed")
	// A malformed archive of the same size is skipped rather than failing the lookup.
	writeInstalled(t, dir, "a.mcpack", bytes.Repeat([]byte{'x'}, len(archive)))
	writeInstalled(t, dir, "user-renamed.MCPACK", archive)
	fallback := newTestCache(t, 1<<20)
	cached, cachedKey, _ := storedPack(t, id, "1.2.3", "fallback!")
	if cachedKey != key {
		t.Fatal("fixture archives must share an identity and size")
	}
	if err := fallback.Store(context.Background(), key, cached); err != nil {
		t.Fatal(err)
	}

	got, err := NewInstalled(dir, fallback).Load(context.Background(), key)
	if err != nil || got == nil {
		t.Fatalf("Load = %v, %v; want installed hit", got, err)
	}
	if got.Checksum() != installedPack.Checksum() {
		t.Fatal("Load returned the fallback archive instead of the installed one")
	}
}

func TestInstalledMissesOtherVersionOrSizeAndFallsBack(t *testing.T) {
	dir := t.TempDir()
	id := uuid.New()
	_, installedKey, archive := storedPack(t, id, "1.2.4", "installed")
	writeInstalled(t, dir, "pack.mcpack", archive)
	cached, key, _ := storedPack(t, id, "1.2.3", "installed")
	if installedKey.Size != key.Size {
		t.Fatal("fixture versions must share a size so only the version differs")
	}
	resized := installedKey
	resized.Size++
	fallback := newTestCache(t, 1<<20)
	if err := fallback.Store(context.Background(), key, cached); err != nil {
		t.Fatal(err)
	}
	installed := NewInstalled(dir, fallback)

	got, err := installed.Load(context.Background(), key)
	if err != nil || got == nil || got.Checksum() != cached.Checksum() {
		t.Fatalf("other version Load = %v, %v; want the fallback archive", got, err)
	}
	if got, err := installed.Load(context.Background(), resized); err != nil || got != nil {
		t.Fatalf("other size Load = %v, %v; want miss", got, err)
	}
}

func TestInstalledToleratesMissingDirectoryAndFallback(t *testing.T) {
	_, key, _ := testPack(t, uuid.New(), "1.0.0", "one")
	installed := NewInstalled(filepath.Join(t.TempDir(), "missing"), nil)
	if got, err := installed.Load(context.Background(), key); err != nil || got != nil {
		t.Fatalf("Load = %v, %v; want miss", got, err)
	}
	if err := installed.Store(context.Background(), key, nil); err != nil {
		t.Fatalf("Store without fallback = %v", err)
	}
	if _, _, _, err := installed.Reference(nil); err == nil {
		t.Fatal("Reference without fallback succeeded")
	}
}

func TestInstalledStoresAndReferencesThroughFallbackOnly(t *testing.T) {
	dir := t.TempDir()
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "one")
	fallback := newTestCache(t, 1<<20)
	installed := NewInstalled(dir, fallback)
	if err := installed.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	if entries, _ := os.ReadDir(dir); len(entries) != 0 {
		t.Fatalf("Store wrote %d entries to the installed directory", len(entries))
	}
	path, checksum, release, err := installed.Reference(pack)
	if err != nil {
		t.Fatalf("Reference = %v", err)
	}
	defer release()
	if checksum != pack.Checksum() || filepath.Dir(path) != fallback.root {
		t.Fatal("Reference did not lend the fallback cache's archive")
	}
	var _ minecraft.ResourcePackCache = installed
}
