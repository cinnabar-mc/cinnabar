package translate

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"os"
	"sort"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
)

// updateFixturesEnv rewrites golden files instead of comparing against them.
const updateFixturesEnv = "CINNABAR_UPDATE_FIXTURES"

// javaColumn builds inflated whole-column chunk data from blocks: block ids and metadata for
// the listed sections, block and sky light, then biomes.
type javaColumn struct {
	blocks map[[3]int]block
	biomes [biomeBytes]byte
}

func newJavaColumn() *javaColumn {
	return &javaColumn{blocks: map[[3]int]block{}}
}

func (c *javaColumn) set(x, y, z int, id uint16, meta uint8) {
	c.blocks[[3]int{x, y, z}] = block{id: id, meta: meta}
}

// data returns the column's primary and add masks and its inflated bytes, laid out for a
// dimension with sky light.
func (c *javaColumn) data() (primary, add uint16, data []byte) {
	for position, value := range c.blocks {
		primary |= 1 << (position[1] >> 4)
		if value.id > 255 {
			add |= 1 << (position[1] >> 4)
		}
	}
	sections := func(mask uint16, size int, fill func(section int, out []byte)) {
		for index := 0; index < sectionCount; index++ {
			if mask&(1<<index) != 0 {
				out := make([]byte, size)
				fill(index, out)
				data = append(data, out...)
			}
		}
	}
	each := func(section int, visit func(index int, value block)) {
		for position, value := range c.blocks {
			if position[1]>>4 == section {
				visit((position[1]&15)<<8|position[2]<<4|position[0], value)
			}
		}
	}
	setNibble := func(out []byte, index int, value uint8) {
		if index&1 == 0 {
			out[index>>1] |= value & 0x0F
		} else {
			out[index>>1] |= value << 4
		}
	}
	sections(primary, sectionVolume, func(section int, out []byte) {
		each(section, func(index int, value block) { out[index] = byte(value.id) })
	})
	sections(primary, nibbleBytes, func(section int, out []byte) {
		each(section, func(index int, value block) { setNibble(out, index, value.meta) })
	})
	sections(primary, nibbleBytes*2, func(int, []byte) {})
	sections(add, nibbleBytes, func(section int, out []byte) {
		each(section, func(index int, value block) { setNibble(out, index, uint8(value.id>>8)) })
	})
	return primary, add, append(data, c.biomes[:]...)
}

func filledWorld(t *testing.T, columns map[chunkPos]*javaColumn) *world {
	t.Helper()
	w := newWorld(dimensionOverworld)
	for pos, column := range columns {
		primary, add, data := column.data()
		if err := w.fill(pos, data, primary, add, true); err != nil {
			t.Fatal(err)
		}
	}
	return w
}

func TestFillReadsBlocksMetadataHighBitsAndBiomes(t *testing.T) {
	column := newJavaColumn()
	column.set(1, 2, 3, 35, 14)
	column.set(15, 20, 0, 300, 5)
	column.set(0, 255, 15, 1, 0)
	column.biomes[3<<4|1] = 6
	w := filledWorld(t, map[chunkPos]*javaColumn{{-1, 2}: column})
	for _, want := range []struct {
		x, y, z int32
		block   block
	}{
		{-16 + 1, 2, 32 + 3, block{35, 14}},
		{-16 + 15, 20, 32, block{300, 5}},
		{-16, 255, 32 + 15, block{1, 0}},
		{-16, 0, 32, block{}},
		{0, 2, 0, block{}},
		{-15, -1, 35, block{}},
		{-15, 256, 35, block{}},
	} {
		if got := w.block(want.x, want.y, want.z); got != want.block {
			t.Errorf("block(%d,%d,%d) = %+v, want %+v", want.x, want.y, want.z, got, want.block)
		}
	}
	if got := w.columns[chunkPos{-1, 2}].biomes[3<<4|1]; got != 6 {
		t.Errorf("biome = %d", got)
	}
}

func TestFillRejectsShortData(t *testing.T) {
	column := newJavaColumn()
	column.set(0, 0, 0, 1, 0)
	primary, add, data := column.data()
	w := newWorld(dimensionOverworld)
	if err := w.fill(chunkPos{}, data[:len(data)-1], primary, add, true); err == nil {
		t.Fatal("short chunk data was accepted")
	}
}

func TestPartialFillKeepsOtherSections(t *testing.T) {
	column := newJavaColumn()
	column.set(0, 0, 0, 1, 0)
	column.set(0, 16, 0, 2, 0)
	w := filledWorld(t, map[chunkPos]*javaColumn{{}: column})
	update := newJavaColumn()
	update.set(0, 16, 0, 3, 0)
	primary, add, data := update.data()
	// A partial update carries no biomes.
	if err := w.fill(chunkPos{}, data[:len(data)-biomeBytes], primary, add, false); err != nil {
		t.Fatal(err)
	}
	if w.block(0, 0, 0).id != 1 || w.block(0, 16, 0).id != 3 {
		t.Fatalf("after a partial update: %+v %+v", w.block(0, 0, 0), w.block(0, 16, 0))
	}
}

// readPaletted decodes a network paletted storage and returns the bytes it consumed.
func readPaletted(t *testing.T, payload []byte) (values [sectionVolume]uint32, consumed int) {
	t.Helper()
	varint := func() int32 {
		value, n := binary.Uvarint(payload[consumed:])
		consumed += n
		return int32(uint32(value)>>1) ^ -int32(value&1)
	}
	header := payload[consumed]
	consumed++
	if header&1 != 1 {
		t.Fatalf("storage header %#x is not a network palette", header)
	}
	bits := int(header >> 1)
	if bits == 0 {
		id := uint32(varint())
		for i := range values {
			values[i] = id
		}
		return values, consumed
	}
	perWord := 32 / bits
	var indices [sectionVolume]uint32
	for start := 0; start < sectionVolume; start += perWord {
		word := binary.LittleEndian.Uint32(payload[consumed:])
		consumed += 4
		for offset := 0; offset < perWord && start+offset < sectionVolume; offset++ {
			indices[start+offset] = word >> (offset * bits) & (1<<bits - 1)
		}
	}
	palette := make([]uint32, varint())
	for i := range palette {
		palette[i] = uint32(varint())
	}
	for i, index := range indices {
		values[i] = palette[index]
	}
	return values, consumed
}

func TestLevelChunkEncodesSectionsAboveTheBedrockFloor(t *testing.T) {
	column := newJavaColumn()
	for x := 0; x < 16; x++ {
		for z := 0; z < 16; z++ {
			column.set(x, 0, z, 1, 0)
		}
	}
	column.set(3, 17, 5, 35, 14)
	for i := range column.biomes {
		column.biomes[i] = 1
	}
	column.biomes[5<<4|3] = 2
	w := filledWorld(t, map[chunkPos]*javaColumn{{4, -3}: column})
	chunk := w.levelChunk(chunkPos{4, -3}, dimensionOverworld)
	if chunk.Position != [2]int32{4, -3} || chunk.Dimension != bedrockOverworld || chunk.SubChunkCount != 6 {
		t.Fatalf("chunk header = %+v %d %d", chunk.Position, chunk.Dimension, chunk.SubChunkCount)
	}
	payload := chunk.RawPayload
	offset := 0
	for index := -4; index < 2; index++ {
		version, layers, y := payload[offset], payload[offset+1], int8(payload[offset+2])
		offset += 3
		if version != subChunkVersion || int(y) != index {
			t.Fatalf("sub-chunk %d header = %d %d %d", index, version, layers, y)
		}
		if index < 0 {
			if layers != 0 {
				t.Fatalf("sub-chunk %d below the Java world has %d layers", index, layers)
			}
			continue
		}
		values, consumed := readPaletted(t, payload[offset:])
		offset += consumed
		at := func(x, y, z int) uint32 { return values[x<<8|z<<4|y] }
		switch index {
		case 0:
			if at(7, 0, 9) != legacy.BlockHash(1, 0) || at(7, 1, 9) != legacy.AirHash() {
				t.Fatalf("floor section holds %d over %d", at(7, 1, 9), at(7, 0, 9))
			}
		case 1:
			if at(3, 1, 5) != legacy.BlockHash(35, 14) {
				t.Fatalf("wool = %d", at(3, 1, 5))
			}
		}
	}
	biomes, consumed := readPaletted(t, payload[offset:])
	offset += consumed
	if biomes[3<<8|5<<4|9] != 2 || biomes[4<<8|5<<4] != 1 {
		t.Fatalf("biomes = %d %d", biomes[3<<8|5<<4|9], biomes[4<<8|5<<4])
	}
	for slot := 1; slot < 24; slot++ {
		if payload[offset] != 0xFF {
			t.Fatalf("biome slot %d header = %#x", slot, payload[offset])
		}
		offset++
	}
	if rest := payload[offset:]; !bytes.Equal(rest, []byte{0}) {
		t.Fatalf("column tail = %x", rest)
	}
}

func TestNetherColumnsKeepTheLowerHalf(t *testing.T) {
	column := newJavaColumn()
	column.set(0, 5, 0, 87, 0)
	column.set(0, 200, 0, 87, 0)
	primary, add, data := column.data()
	// The nether has no sky light array.
	w := newWorld(dimensionNether)
	withoutSky := append([]byte{}, data[:len(data)-biomeBytes-2*nibbleBytes*2]...)
	withoutSky = append(withoutSky, make([]byte, 2*nibbleBytes)...)
	withoutSky = append(withoutSky, column.biomes[:]...)
	if err := w.fill(chunkPos{}, withoutSky, primary, add, true); err != nil {
		t.Fatal(err)
	}
	chunk := w.levelChunk(chunkPos{}, dimensionNether)
	if chunk.Dimension != bedrockNether || chunk.SubChunkCount != 1 {
		t.Fatalf("nether chunk = dimension %d, %d sub-chunks", chunk.Dimension, chunk.SubChunkCount)
	}
}

func TestFencesPanesAndWallsConnect(t *testing.T) {
	column := newJavaColumn()
	// A fence with stone to the east, a gourd to the west and a fence gate to the south.
	column.set(5, 1, 5, blockFence, 0)
	column.set(6, 1, 5, 1, 0)
	column.set(4, 1, 5, 86, 0)
	column.set(5, 1, 6, blockFenceGate, 0)
	// A pane beside glass and leaves; a wall running north to south, then one with a corner.
	column.set(10, 1, 10, blockGlassPane, 0)
	column.set(10, 1, 9, blockGlass, 0)
	column.set(11, 1, 10, 18, 0)
	column.set(2, 1, 12, blockCobblestoneWall, 0)
	column.set(2, 1, 11, blockCobblestoneWall, 0)
	column.set(2, 1, 13, blockCobblestoneWall, 0)
	column.set(3, 1, 13, 1, 0)
	w := filledWorld(t, map[chunkPos]*javaColumn{{}: column})
	for _, want := range []struct {
		x, y, z int32
		state   string
	}{
		{5, 1, 5, "minecraft:oak_fence[minecraft:connection_east=true,minecraft:connection_north=false,minecraft:connection_south=true,minecraft:connection_west=false]"},
		{10, 1, 10, "minecraft:glass_pane[minecraft:connection_east=true,minecraft:connection_north=true,minecraft:connection_south=false,minecraft:connection_west=false]"},
		{2, 1, 12, "minecraft:cobblestone_wall[wall_connection_type_east=none,wall_connection_type_north=short,wall_connection_type_south=short,wall_connection_type_west=none,wall_post_bit=false]"},
		{2, 1, 13, "minecraft:cobblestone_wall[wall_connection_type_east=short,wall_connection_type_north=short,wall_connection_type_south=none,wall_connection_type_west=none,wall_post_bit=true]"},
	} {
		state := w.state(want.x, want.y, want.z)
		if got := state.String(); got != want.state {
			t.Errorf("state(%d,%d,%d) = %s\nwant %s", want.x, want.y, want.z, got, want.state)
		}
		if w.networkID(want.x, want.y, want.z) != state.NetworkHash() {
			t.Errorf("networkID(%d,%d,%d) does not identify its state", want.x, want.y, want.z)
		}
	}
}

func TestStairsFormCorners(t *testing.T) {
	// Metadata 0 rises towards the east, 2 towards the south and 3 towards the north.
	for _, test := range []struct {
		name      string
		neighbour [3]int
		meta      uint8
		corner    string
	}{
		{"alone", [3]int{}, 0, "none"},
		{"turned stair in front", [3]int{6, 1, 5}, 2, "outer_right"},
		{"turned stair in front, other way", [3]int{6, 1, 5}, 3, "outer_left"},
		{"turned stair behind", [3]int{4, 1, 5}, 2, "inner_right"},
		{"turned stair behind, other way", [3]int{4, 1, 5}, 3, "inner_left"},
		{"same facing in front", [3]int{6, 1, 5}, 0, "none"},
		{"upside-down stair in front", [3]int{6, 1, 5}, 2 | 4, "none"},
	} {
		column := newJavaColumn()
		column.set(5, 1, 5, 53, 0)
		if test.neighbour != [3]int{} {
			column.set(test.neighbour[0], test.neighbour[1], test.neighbour[2], 67, test.meta)
		}
		w := filledWorld(t, map[chunkPos]*javaColumn{{}: column})
		if got := w.state(5, 1, 5).Properties["minecraft:corner"]; got != test.corner {
			t.Errorf("%s: corner = %v, want %s", test.name, got, test.corner)
		}
	}
}

// Every connection variant the translator can produce is listed in testdata, where the
// client's registry test checks that each state exists.
func TestConnectedStatesGolden(t *testing.T) {
	states := map[string]uint32{}
	record := func(current block, variant uint8, state resolvedState) {
		states[state.state.String()] = state.hash
	}
	for id := uint16(0); id <= legacy.MaxBlockID; id++ {
		if !connects(id) {
			continue
		}
		for meta := uint8(0); meta < 16; meta++ {
			current := block{id: id, meta: meta}
			switch {
			case isStairs(id):
				for corner := range stairCorners {
					record(current, uint8(corner), withProperties(current, uint8(corner), func(set func(string, any)) {
						set("minecraft:corner", stairCorners[corner])
					}))
				}
			default:
				for sides := uint8(0); sides < 16; sides++ {
					w := newWorld(dimensionOverworld)
					w.columns[chunkPos{}] = &column{}
					w.set(5, 1, 5, current)
					for index, side := range horizontal {
						if sides&(1<<index) != 0 {
							w.set(5+side.dx, 1, 5+side.dz, block{id: 1})
						}
					}
					resolved, ok := w.connected(5, 1, 5, current)
					if !ok {
						t.Fatalf("%d:%d does not connect", id, meta)
					}
					record(current, sides, resolved)
				}
			}
		}
	}
	lines := make([]string, 0, len(states))
	for state, hash := range states {
		lines = append(lines, fmt.Sprintf("%s\t%d", state, hash))
	}
	sort.Strings(lines)
	golden := "# Bedrock states the Java translator produces for neighbour-dependent blocks.\n# state\tnetwork hash\n" + strings.Join(lines, "\n") + "\n"
	const path = "testdata/connected_states.tsv"
	if os.Getenv(updateFixturesEnv) != "" {
		if err := os.WriteFile(path, []byte(golden), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	current, err := os.ReadFile(path)
	if err != nil || string(current) != golden {
		t.Fatalf("%s is stale; rerun with %s=1 (%v)", path, updateFixturesEnv, err)
	}
}

// A server that keeps sending new columns cannot grow the store: the farthest are dropped.
func TestStoreKeepsTheNearestColumns(t *testing.T) {
	w := newWorld(dimensionOverworld)
	for x := int32(0); x < maxColumns+50; x++ {
		pos := chunkPos{x, 0}
		w.columns[pos] = &column{}
		w.evict(chunkPos{}, pos)
	}
	if len(w.columns) != maxColumns {
		t.Fatalf("store holds %d columns", len(w.columns))
	}
	if w.columns[chunkPos{0, 0}] == nil || w.columns[chunkPos{maxColumns + 49, 0}] == nil {
		t.Fatal("the nearest column or the one just stored was dropped")
	}
	if w.columns[chunkPos{maxColumns + 40, 0}] != nil {
		t.Fatal("a far column outlived nearer ones")
	}
}
