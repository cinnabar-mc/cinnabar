package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// The tint table must load when the generator runs from the repository root or below it.
func TestBeaconTintTableLoadsFromAnyCheckoutDirectory(t *testing.T) {
	for _, start := range []string{filepath.Join("..", ".."), ".", "cmd"} {
		tints, err := loadBeaconTints(start)
		if err != nil {
			t.Fatalf("load from %s: %v", start, err)
		}
		if len(tints) != len(beaconTints) {
			t.Fatalf("load from %s: got %d tints, want %d", start, len(tints), len(beaconTints))
		}
	}
}

func TestGlassStatesHaveCompiledBeaconTints(t *testing.T) {
	data, err := os.ReadFile("../../crates/assets/data/block-registry-v2225.bin")
	if err != nil {
		t.Fatal(err)
	}
	_, records, err := decodeBREGRecords(data, v2225BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	// The independent canonical palette supplies the complete admitted identity inventory.
	source, err := v2225SourceRecords()
	if err != nil {
		t.Fatal(err)
	}
	byHash := make(map[uint32]Record, len(records))
	for _, record := range records {
		byHash[record.NetworkHash] = record
	}
	for _, state := range source {
		if !strings.HasSuffix(state.Name, "_stained_glass") && !strings.HasSuffix(state.Name, "_stained_glass_pane") {
			continue
		}
		expected, found := beaconTints[state.Name]
		if !found {
			t.Errorf("admitted glass %s has no eligibility descriptor", state.Name)
			continue
		}
		if actual, found := byHash[state.NetworkHash]; !found || actual.BeaconTint != expected {
			t.Errorf("glass %s %s: got tint %d, want %d", state.Name, state.StateJSON, actual.BeaconTint, expected)
		}
	}
}
