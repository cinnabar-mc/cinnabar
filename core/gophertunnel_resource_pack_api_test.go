package core_test

import (
	"archive/zip"
	"bytes"
	"context"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type resourcePackCacheStub struct{}

func (*resourcePackCacheStub) Load(context.Context, minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	return nil, nil
}

func (*resourcePackCacheStub) Store(context.Context, minecraft.ResourcePackCacheKey, *resource.Pack) error {
	return nil
}

func TestGophertunnelResourcePackTransportAPI(t *testing.T) {
	var _ minecraft.ResourcePackCache = (*resourcePackCacheStub)(nil)
	var _ minecraft.ResourcePackCache = minecraft.DirResourcePackCache{}
	var _ func(*minecraft.Conn, []*resource.Pack, bool) error = (*minecraft.Conn).ConfigureResourcePackOffer
	var _ func(*minecraft.Conn) []*resource.Pack = (*minecraft.Conn).ResourcePacks
	var _ func(*minecraft.Conn) (minecraft.ResourcePackOfferSnapshot, bool) = (*minecraft.Conn).ResourcePackOffer
	var _ func(*minecraft.Conn, minecraft.ResourcePackOfferSnapshot, bool) error = (*minecraft.Conn).ConfigureResourcePackOfferSnapshot
	var _ func(*minecraft.Conn) bool = (*minecraft.Conn).TexturePacksRequired
	var _ func(*minecraft.Conn) (minecraft.ResourcePackStackSnapshot, bool) = (*minecraft.Conn).ResourcePackStack
	var _ func(*minecraft.Conn, minecraft.ResourcePackStackSnapshot, bool) error = (*minecraft.Conn).ConfigureResourcePackStack
	var _ func(minecraft.ResourcePackStackSnapshot) []minecraft.ResourcePackStackEntry = minecraft.ResourcePackStackSnapshot.Entries
	var _ func(minecraft.ResourcePackStackSnapshot) []*resource.Pack = minecraft.ResourcePackStackSnapshot.Packs
	var _ func(minecraft.ResourcePackStackSnapshot) bool = minecraft.ResourcePackStackSnapshot.Required
	var _ func(minecraft.ResourcePackStackSnapshot) string = minecraft.ResourcePackStackSnapshot.BaseGameVersion
	var _ func(minecraft.ResourcePackStackSnapshot) []protocol.ExperimentData = minecraft.ResourcePackStackSnapshot.Experiments
	var _ func(minecraft.ResourcePackStackSnapshot) bool = minecraft.ResourcePackStackSnapshot.ExperimentsPreviouslyToggled
	var _ func(minecraft.ResourcePackStackSnapshot) bool = minecraft.ResourcePackStackSnapshot.IncludeEditorPacks
	var _ func(minecraft.ResourcePackStackEntry) *resource.Pack = minecraft.ResourcePackStackEntry.Pack
	var _ func(minecraft.ResourcePackStackEntry) string = minecraft.ResourcePackStackEntry.UUID
	var _ func(minecraft.ResourcePackStackEntry) string = minecraft.ResourcePackStackEntry.Version
	var _ func(minecraft.ResourcePackStackEntry) string = minecraft.ResourcePackStackEntry.SubPackName
	var _ func(minecraft.ResourcePackOfferSnapshot) []minecraft.ResourcePackOfferEntry = minecraft.ResourcePackOfferSnapshot.TexturePacks
	var _ func(minecraft.ResourcePackOfferEntry) protocol.TexturePackInfo = minecraft.ResourcePackOfferEntry.Info
	var _ func(minecraft.ResourcePackOfferEntry) *resource.Pack = minecraft.ResourcePackOfferEntry.Pack
	var _ func(*resource.Pack) *resource.Pack = (*resource.Pack).Clone

	download := minecraft.ResourcePackDownloadConfig{
		MaxInFlightChunks: 7,
	}
	cache := &resourcePackCacheStub{}
	dialer := minecraft.Dialer{ResourcePackDownload: download, ResourcePackCache: cache}
	listener := minecraft.ListenConfig{
		TexturePacksRequired: true,
		ResourcePackDelivery: minecraft.ResourcePackDeliveryConfig{ChunkSize: 13},
		PrepareResourcePackOffer: func(_ context.Context, conn *minecraft.Conn) error {
			return conn.ConfigureResourcePackOffer(nil, true)
		},
	}
	key := minecraft.ResourcePackCacheKey{
		UUID: uuid.Nil, Version: "1.0.0", Size: 14,
	}

	if dialer.ResourcePackDownload != download || dialer.ResourcePackCache != cache {
		t.Fatal("Dialer did not retain resource-pack transport configuration")
	}
	if !listener.TexturePacksRequired || listener.PrepareResourcePackOffer == nil || key.Version != "1.0.0" ||
		key.Size != 14 {
		t.Fatal("resource-pack offer and cache-key API drifted")
	}
	if minecraft.DefaultResourcePackMaxInFlightChunks <= 0 || minecraft.DefaultResourcePackChunkSize == 0 {
		t.Fatal("resource-pack transport defaults must remain bounded and non-zero")
	}
}

// TestGophertunnelReadsRootPackManifest pins the fork's manifest lookup: a pack whose archive lists an
// asset folder's manifest.json before its root manifest, as OneBlock's Marketplace pack does, keeps
// the root manifest's identity so its URL download matches the advertised pack.
func TestGophertunnelReadsRootPackManifest(t *testing.T) {
	const root = `{"format_version": 2, "header": {"name": "p", "uuid": "557aceac-4555-4734-90a7-325380966c3c", "version": [1, 8, 71]},
"modules": [{"type": "resources", "uuid": "557aceac-4555-4734-90a7-325380966c3d", "version": [1, 8, 71]}]}`
	var archive bytes.Buffer
	zw := zip.NewWriter(&archive)
	for _, entry := range [][2]string{
		{"textures/entity/aquatic_mobs/manifest.json", `{"frames": []}`},
		{"manifest.json", root},
	} {
		w, err := zw.Create(entry[0])
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Write([]byte(entry[1])); err != nil {
			t.Fatal(err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	pack, err := resource.ReadBytes(archive.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	if pack.UUID() != uuid.MustParse("557aceac-4555-4734-90a7-325380966c3c") || pack.Version() != "1.8.71" {
		t.Fatalf("pack = %s_%s, want the root manifest's 557aceac-4555-4734-90a7-325380966c3c_1.8.71", pack.UUID(), pack.Version())
	}
}
