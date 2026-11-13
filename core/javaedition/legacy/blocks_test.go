package legacy

import "testing"

func TestTableLoads(t *testing.T) {
	if _, err := loadBlocks(); err != nil {
		t.Fatal(err)
	}
}

func TestParseStateRoundTrips(t *testing.T) {
	const text = "minecraft:oak_stairs[upside_down_bit=true,weirdo_direction=1]"
	state, err := ParseState(text)
	if err != nil {
		t.Fatal(err)
	}
	if got := state.String(); got != text {
		t.Fatalf("String() = %q, want %q", got, text)
	}
	if state.Properties["upside_down_bit"] != true || state.Properties["weirdo_direction"] != int32(1) {
		t.Fatalf("properties = %#v", state.Properties)
	}
}

func TestParseStateRejectsMalformedText(t *testing.T) {
	for _, text := range []string{"", "[a=b]", "minecraft:stone[a", "minecraft:stone[a]", "minecraft:stone[a=1,a=2]"} {
		if _, err := ParseState(text); err == nil {
			t.Errorf("ParseState(%q) succeeded", text)
		}
	}
}

// The air hash is the FNV-1a 32 of the NBT compound {name: "minecraft:air", states: {}},
// which the Bedrock registry pins.
func TestNetworkHashOfAir(t *testing.T) {
	state, err := ParseState("minecraft:air[]")
	if err != nil {
		t.Fatal(err)
	}
	if got := state.NetworkHash(); got != 3690217760 {
		t.Fatalf("air hash = %d", got)
	}
	if AirHash() != 3690217760 {
		t.Fatalf("AirHash() = %d", AirHash())
	}
}

func TestBlockFallbacks(t *testing.T) {
	stone := Block(1, 0)
	if stone.Name != "minecraft:stone" {
		t.Fatalf("stone = %s", stone)
	}
	// 1.7.10 has one stone; later metadata values draw the same block.
	if got := Block(1, 3); got.Name != "minecraft:stone" {
		t.Errorf("stone metadata 3 = %s", got)
	}
	// Metadata keeps its low four bits, as chunk nibbles do.
	if got, want := BlockHash(35, 14+16), BlockHash(35, 14); got != want {
		t.Errorf("metadata was not masked: %d != %d", got, want)
	}
	for _, id := range []uint16{165, 169, 176, 4095} {
		if got := BlockHash(id, 0); got != AirHash() {
			t.Errorf("unregistered id %d = %d, want air", id, got)
		}
	}
	if got := Block(35, 14); got.Name != "minecraft:red_wool" {
		t.Errorf("35:14 = %s", got)
	}
}

func TestEveryMetadataOfARegisteredBlockResolvesToIt(t *testing.T) {
	for id := 1; id <= MaxBlockID; id++ {
		for meta := 0; meta < metaCount; meta++ {
			isAir := BlockHash(uint16(id), uint8(meta)) == AirHash()
			if Registered(id) == isAir {
				t.Errorf("%d:%d: registered %v, air %v", id, meta, Registered(id), isAir)
			}
		}
	}
	// A torch lists metadata 1 to 5 only; metadata 0 still resolves to a torch.
	if got := Block(50, 0); got.Name != "minecraft:torch" {
		t.Errorf("50:0 = %s", got)
	}
}
