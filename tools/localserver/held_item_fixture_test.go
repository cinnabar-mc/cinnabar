package main

import (
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// A declared spec's registry entry gains the component; others stay on the default grip.
func TestWithHeldItemOffsetsAddsOnlyDeclaredComponents(t *testing.T) {
	entry := func(name string) protocol.ItemEntry {
		return protocol.ItemEntry{Name: name, Data: map[string]any{"components": map[string]any{}}}
	}
	entries := withHeldItemOffsets([]protocol.ItemEntry{
		entry("fixture:held_tool"),
		entry("fixture:held_scaled"),
		entry("fixture:held_preset"),
		entry("minecraft:apple"),
	})
	declared := func(index int) any {
		return entries[index].Data["components"].(map[string]any)[renderOffsetsComponent]
	}
	if declared(0) != nil || declared(3) != nil {
		t.Fatalf("default grip items gained offsets: %v, %v", declared(0), declared(3))
	}
	if _, ok := declared(1).(map[string]any); !ok {
		t.Fatalf("scaled item offsets = %v", declared(1))
	}
	if declared(2) != "tools" {
		t.Fatalf("preset item offsets = %v", declared(2))
	}
}

// Icons must keep the declared texel size: render offsets depend on it.
func TestHeldItemFixtureTextureHasDeclaredSize(t *testing.T) {
	for index, spec := range heldItemFixtureSpecs {
		if got := (heldItemFixture{spec: index}).Texture().Bounds().Dx(); got != spec.texels {
			t.Fatalf("%s texture is %d texels, want %d", spec.id, got, spec.texels)
		}
	}
}
