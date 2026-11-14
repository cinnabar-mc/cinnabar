package legacy

import (
	"bytes"
	"compress/gzip"
	"io"
	"os"
	"testing"
)

// The simulation crate owns the recorded block facts; this package embeds a copy because Go
// cannot embed files outside its module.
func TestBlockFactsMatchTheSimulationFixture(t *testing.T) {
	const fixture = "../../../crates/java-sim/fixtures/blocks.jsonl.gz"
	compressed, err := os.ReadFile(fixture)
	if err != nil {
		t.Skipf("missing fixture: %s", fixture)
	}
	reader, err := gzip.NewReader(bytes.NewReader(compressed))
	if err != nil {
		t.Fatal(err)
	}
	recorded, err := io.ReadAll(reader)
	if err != nil {
		t.Fatal(err)
	}
	if string(recorded) != blockFacts {
		t.Fatal("blockfacts.jsonl differs from crates/java-sim/fixtures/blocks.jsonl.gz; copy the fixture's contents")
	}
}

func TestBlockFacts(t *testing.T) {
	if facts := BlockFacts(1); !facts.NormalCube || !facts.Opaque || facts.Material != "rock" {
		t.Errorf("stone = %+v", facts)
	}
	if facts := BlockFacts(86); facts.Material != "gourd" {
		t.Errorf("pumpkin = %+v", facts)
	}
	if facts := BlockFacts(20); facts.Opaque && facts.RenderNormal && facts.FullBlock {
		t.Errorf("glass = %+v", facts)
	}
	for _, id := range []uint16{165, 4000} {
		if facts := BlockFacts(id); facts.Material != "air" {
			t.Errorf("unregistered id %d = %+v", id, facts)
		}
	}
}

func TestItemTables(t *testing.T) {
	if _, err := loadItems(); err != nil {
		t.Fatal(err)
	}
	if len(Registry()) < 1000 {
		t.Fatalf("registry holds %d items", len(Registry()))
	}
	sword, variant, ok := JavaItem(276, 0)
	if !ok || !variant || sword.Name != "minecraft:diamond_sword" {
		t.Errorf("276:0 = %+v %v %v", sword, variant, ok)
	}
	// A worn sword has no table row of its own: the damage is wear.
	if worn, variant, ok := JavaItem(276, 100); !ok || variant || worn.Name != "minecraft:diamond_sword" {
		t.Errorf("276:100 = %+v %v %v", worn, variant, ok)
	}
	if dye, variant, ok := JavaItem(351, 4); !ok || !variant || dye.Name != "minecraft:lapis_lazuli" {
		t.Errorf("351:4 = %+v %v %v", dye, variant, ok)
	}
	if _, _, ok := JavaItem(409, 0); ok {
		t.Error("409 is not a 1.7.10 item")
	}
	for _, item := range Registry() {
		if _, err := item.Data(); err != nil {
			t.Errorf("%s: %v", item.Name, err)
		}
	}
	if id, ok := ItemID("minecraft:apple"); !ok || id != 883 {
		t.Errorf("apple id = %d %v", id, ok)
	}
}
