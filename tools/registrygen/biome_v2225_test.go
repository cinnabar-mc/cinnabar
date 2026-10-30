package main

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	sharedbiome "github.com/cinnabar-mc/protocolgen/generated/data/biome"
)

// TestV2225BiomeProjectionRejectsInvalidSharedIDs covers invalid or ambiguous shared records.
func TestV2225BiomeProjectionRejectsInvalidSharedIDs(t *testing.T) {
	tests := []struct {
		name    string
		mutate  func([]sharedbiome.Biome)
		wantErr string
	}{
		{name: "missing ID", mutate: func(source []sharedbiome.Biome) { source[0].HasID = false }, wantErr: "no valid uint16 ID"},
		{name: "negative ID", mutate: func(source []sharedbiome.Biome) { source[0].ID = -1 }, wantErr: "no valid uint16 ID"},
		{name: "large ID", mutate: func(source []sharedbiome.Biome) { source[0].ID = 1 << 16 }, wantErr: "no valid uint16 ID"},
		{name: "duplicate ID", mutate: func(source []sharedbiome.Biome) { source[1].ID = source[0].ID }, wantErr: "duplicate shared biome ID"},
		{name: "duplicate name", mutate: func(source []sharedbiome.Biome) { source[1].Name = source[0].Name }, wantErr: "duplicate shared biome name"},
		{name: "invalid name", mutate: func(source []sharedbiome.Biome) { source[0].Name = "minecraft:\xff" }, wantErr: "invalid name"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			source, allowed := syntheticV2225Projection()
			test.mutate(source)
			if _, _, err := projectV2225BiomeRecords(source, allowed); err == nil || !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("error = %v, want %q", err, test.wantErr)
			}
		})
	}
}

// TestV2225BiomeProjectionIsDefaultDeny keeps local admission separate from catalog coverage.
func TestV2225BiomeProjectionIsDefaultDeny(t *testing.T) {
	source, allowed := syntheticV2225Projection()
	source = append(source, sharedbiome.Biome{Name: "minecraft:excluded", ID: 1000, HasID: true})
	projected, stats, err := projectV2225BiomeRecords(source, allowed)
	if err != nil {
		t.Fatal(err)
	}
	if len(projected) != v2225RetailBiomeCount || stats.SourceCount != len(source) || stats.IgnoredCount != 1 || len(stats.IgnoredFingerprint) != 64 {
		t.Fatalf("projection=%d stats=%+v", len(projected), stats)
	}
	if !sort.SliceIsSorted(projected, func(i, j int) bool { return projected[i].ID < projected[j].ID }) {
		t.Fatal("projection is not sorted by numeric ID")
	}
	for _, record := range projected {
		if _, ok := allowed[record.Name]; !ok {
			t.Fatalf("admitted biome %q outside the allowlist", record.Name)
		}
	}
	source[0].Name = "minecraft:missing_retail_name"
	if _, _, err := projectV2225BiomeRecords(source, allowed); err == nil || !strings.Contains(err.Error(), "missing") {
		t.Fatalf("missing retained name error = %v", err)
	}
	delete(allowed, source[1].Name)
	if _, _, err := projectV2225BiomeRecords(source, allowed); err == nil || !strings.Contains(err.Error(), "count") {
		t.Fatalf("incorrect allowlist scope error = %v", err)
	}
}

// TestV2225BiomeAllowlistRejectsChanges protects the reviewed local retail scope.
func TestV2225BiomeAllowlistRejectsChanges(t *testing.T) {
	if _, err := parseV2225BiomeAllowlist([]byte("minecraft:only_one\n")); err == nil {
		t.Fatal("accepted incomplete allowlist")
	}
}

// TestV2225BiomeSharedCatalogReproducesCarrier checks the full projection without external inputs.
func TestV2225BiomeSharedCatalogReproducesCarrier(t *testing.T) {
	root := filepath.Join("..", "..")
	dir := t.TempDir()
	output := filepath.Join(dir, "biomes.bin")
	manifest := filepath.Join(dir, "biomes.json")
	if err := writeV2225BiomeProjection(filepath.Join(root, v2225BiomeAllowlistPath), output, manifest); err != nil {
		t.Fatal(err)
	}
	for generated, checkedIn := range map[string]string{
		output: filepath.Join(root, v2225BiomeOutputPath),
		strings.TrimSuffix(output, ".bin") + ".sha256": filepath.Join(root, strings.TrimSuffix(v2225BiomeOutputPath, ".bin")+".sha256"),
		manifest: filepath.Join(root, v2225BiomeProjectionPath),
	} {
		got, err := os.ReadFile(generated)
		if err != nil {
			t.Fatal(err)
		}
		want, err := os.ReadFile(checkedIn)
		if err != nil {
			t.Fatal(err)
		}
		// Normalize checkout line endings only for JSON, retaining exact carrier checks.
		if strings.HasSuffix(checkedIn, ".json") {
			want = bytes.ReplaceAll(want, []byte("\r\n"), []byte("\n"))
		}
		if !bytes.Equal(got, want) {
			t.Errorf("shared biome projection differs from %s", checkedIn)
		}
	}
}

// syntheticV2225Projection supplies unique IDs including zero in an unsorted retail scope.
func syntheticV2225Projection() ([]sharedbiome.Biome, map[string]struct{}) {
	source := make([]sharedbiome.Biome, 0, v2225RetailBiomeCount)
	allowed := make(map[string]struct{}, v2225RetailBiomeCount)
	for index := range v2225RetailBiomeCount {
		name := fmt.Sprintf("minecraft:test_%03d", index)
		source = append(source, sharedbiome.Biome{Name: name, ID: int32(v2225RetailBiomeCount - index - 1), HasID: true})
		allowed[name] = struct{}{}
	}
	return source, allowed
}
