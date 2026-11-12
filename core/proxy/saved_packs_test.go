package proxy

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

func createTestPackArchive(t *testing.T, packUUID, version string, extraBytes int) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	manifest := fmt.Sprintf(`{
		"format_version": 2,
		"header": {
			"name": "Test Pack",
			"description": "Test Pack Description",
			"uuid": "%s",
			"version": [1, 0, 0],
			"min_engine_version": [1, 20, 0]
		},
		"modules": [
			{
				"type": "resources",
				"uuid": "%s",
				"version": [1, 0, 0]
			}
		]
	}`, packUUID, uuid.NewString())

	mw, err := zw.Create("manifest.json")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := mw.Write([]byte(manifest)); err != nil {
		t.Fatal(err)
	}

	if extraBytes > 0 {
		ew, err := zw.Create("textures/custom_texture.png")
		if err != nil {
			t.Fatal(err)
		}
		if _, err := ew.Write(make([]byte, extraBytes)); err != nil {
			t.Fatal(err)
		}
	}

	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func TestFindLocalMatchingPack(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "00112233-4455-6677-8899-aabbccddeeff"
	packBytes := createTestPackArchive(t, packUUID, "1.0.0", 500)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, packBytes, 0644); err != nil {
		t.Fatal(err)
	}

	foundPack, foundPath, err := findLocalMatchingPack(packUUID)
	if err != nil {
		t.Fatalf("expected to find local matching pack, got err: %v", err)
	}
	if foundPack.UUID().String() != packUUID {
		t.Fatalf("expected UUID %s, got %s", packUUID, foundPack.UUID().String())
	}
	if foundPath != packPath {
		t.Fatalf("expected path %s, got %s", packPath, foundPath)
	}

	// Test uppercase UUID search
	foundUpper, _, err := findLocalMatchingPack("00112233-4455-6677-8899-AABBCCDDEEFF")
	if err != nil || foundUpper == nil {
		t.Fatalf("expected uppercase UUID match, got err: %v", err)
	}
}

func TestServerPackManifestSyncBypassesDownload(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "12345678-abcd-ef01-2345-6789abcdef01"
	// Modified local pack is larger (1500 bytes extra) than the server advertised size (100 bytes)
	localPackBytes := createTestPackArchive(t, packUUID, "1.0.0", 1500)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, localPackBytes, 0644); err != nil {
		t.Fatal(err)
	}

	parsedUUID := uuid.MustParse(packUUID)
	budget := newResourcePackAcquisitionBudget(nil, nil)

	info := &packet.ResourcePacksInfo{
		TexturePackRequired: true,
		TexturePacks: []protocol.TexturePackInfo{
			{
				UUID:        parsedUUID,
				Version:     "1.0.0",
				Size:        100, // Server advertises 100 bytes, but local modified pack is > 1500 bytes
				ContentKey:  "",
				SubPackName: "",
			},
		},
	}

	budget.admitOffer(info, true)

	// Since local pack matches manifest UUID, downloading from server should be bypassed (accepted[0] == false)
	if budget.accepted[0] {
		t.Fatalf("expected download to be bypassed (accepted=false), got accepted=true")
	}
	if budget.matchedLocal[packUUID] == nil {
		t.Fatalf("expected matchedLocal to store the local pack")
	}

	// Verify that budget.excludes does NOT exclude the modified pack even though its size is > offered (100)
	localPack, err := resource.ReadBytes(localPackBytes)
	if err != nil {
		t.Fatal(err)
	}
	if budget.excludes(localPack) {
		t.Fatalf("budget.excludes rejected modified local pack with size %d > offered 100", localPack.Size())
	}
}

func TestChooseSessionPacksWithLocalMatchingPack(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "99887766-5544-3322-1100-ffeeddccbbaa"
	localPackBytes := createTestPackArchive(t, packUUID, "2.0.0", 200)
	packPath := filepath.Join(tempDir, packUUID+"-2.0.0-1.mcpack")
	if err := os.WriteFile(packPath, localPackBytes, 0644); err != nil {
		t.Fatal(err)
	}

	parsedUUID := uuid.MustParse(packUUID)
	// Server offered the pack, but gophertunnel pack was nil (bypassed download)
	offers := []sessionOffer{
		{
			info: protocol.TexturePackInfo{
				UUID:    parsedUUID,
				Version: "2.0.0",
			},
			pack: nil, // was bypassed
		},
	}
	entries := []sessionStackEntry{
		{
			uuid:    packUUID,
			version: "2.0.0",
		},
	}

	selected, packs, err := chooseSessionPacks(offers, entries, true, nil)
	if err != nil {
		t.Fatalf("chooseSessionPacks failed with local matching pack: %v", err)
	}
	if len(selected) != 1 || len(packs) != 1 {
		t.Fatalf("expected 1 pack selected, got %d", len(selected))
	}
	if selected[0].UUID != packUUID {
		t.Fatalf("expected selected pack UUID %s, got %s", packUUID, selected[0].UUID)
	}
	if packs[0] == nil || packs[0].UUID().String() != packUUID {
		t.Fatalf("expected pack archive to be loaded from local pack")
	}
}

func TestPrependSavedPacks(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "00112233-4455-6677-8899-aabbccddeeff"
	packBytes := createTestPackArchive(t, packUUID, "1.0.0", 50)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, packBytes, 0644); err != nil {
		t.Fatal(err)
	}

	config := globalPacksConfig{
		Active: []struct {
			ID       string `json:"id"`
			SubPack  string `json:"subpack"`
			Revision uint64 `json:"revision"`
		}{
			{
				ID:       packUUID,
				SubPack:  "custom_subpack",
				Revision: 1,
			},
		},
	}
	configData, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tempDir, "global_packs.json"), configData, 0644); err != nil {
		t.Fatal(err)
	}

	serverPackUUID := "11223344-5566-7788-99aa-bbccddeeff00"
	serverPackBytes := createTestPackArchive(t, serverPackUUID, "1.0.0", 50)
	serverPack, err := resource.ReadBytes(serverPackBytes)
	if err != nil {
		t.Fatal(err)
	}

	initialSelected := []sessionPack{
		{
			UUID:    serverPackUUID,
			Version: serverPack.Version(),
			Size:    uint64(len(serverPackBytes)),
		},
	}
	initialPacks := []*resource.Pack{serverPack}

	newSelected, newPacks := prependSavedPacks(initialSelected, initialPacks, nil)
	if len(newSelected) != 2 || len(newPacks) != 2 {
		t.Fatalf("expected 2 packs, got %d selected and %d packs", len(newSelected), len(newPacks))
	}

	if newSelected[0].UUID != packUUID {
		t.Fatalf("expected first selected pack to be %s, got %s", packUUID, newSelected[0].UUID)
	}
	if newSelected[0].SubPack != "custom_subpack" {
		t.Fatalf("expected subpack custom_subpack, got %s", newSelected[0].SubPack)
	}
	if newSelected[1].UUID != serverPackUUID {
		t.Fatalf("expected second selected pack to be %s, got %s", serverPackUUID, newSelected[1].UUID)
	}
}

func TestPrependSavedPacksDeduplication(t *testing.T) {
	tempDir := t.TempDir()
	globalPacksDirOverride = tempDir
	defer func() { globalPacksDirOverride = "" }()

	packUUID := "00112233-4455-6677-8899-aabbccddeeff"
	packBytes := createTestPackArchive(t, packUUID, "1.0.0", 50)
	packPath := filepath.Join(tempDir, packUUID+"-1.0.0-1.mcpack")
	if err := os.WriteFile(packPath, packBytes, 0644); err != nil {
		t.Fatal(err)
	}

	config := globalPacksConfig{
		Active: []struct {
			ID       string `json:"id"`
			SubPack  string `json:"subpack"`
			Revision uint64 `json:"revision"`
		}{
			{
				ID:       packUUID,
				SubPack:  "",
				Revision: 1,
			},
		},
	}
	configData, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tempDir, "global_packs.json"), configData, 0644); err != nil {
		t.Fatal(err)
	}

	// Server also has the same pack UUID
	serverPack, err := resource.ReadBytes(packBytes)
	if err != nil {
		t.Fatal(err)
	}
	initialSelected := []sessionPack{
		{
			UUID:    packUUID,
			Version: serverPack.Version(),
			Size:    uint64(len(packBytes)),
		},
	}
	initialPacks := []*resource.Pack{serverPack}

	newSelected, newPacks := prependSavedPacks(initialSelected, initialPacks, nil)
	if len(newSelected) != 1 || len(newPacks) != 1 {
		t.Fatalf("expected 1 deduplicated pack, got %d selected and %d packs", len(newSelected), len(newPacks))
	}
	if newSelected[0].UUID != packUUID {
		t.Fatalf("expected pack UUID %s, got %s", packUUID, newSelected[0].UUID)
	}
}
