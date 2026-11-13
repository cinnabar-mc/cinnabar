package translate

import (
	"errors"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
)

const (
	// sectionCount is the number of 16-block sections in a 1.7.10 column.
	sectionCount = 16
	// sectionVolume is the number of blocks in a section.
	sectionVolume = 4096
	// nibbleBytes is the size of a section's 4-bit array.
	nibbleBytes = sectionVolume / 2
	// biomeBytes is the size of a column's biome array.
	biomeBytes = 256
	// worldHeight is the first block Y above a 1.7.10 column.
	worldHeight = sectionCount * 16
)

// Java dimension ids.
const (
	dimensionNether    = -1
	dimensionOverworld = 0
	dimensionEnd       = 1
)

// block is a 1.7.10 block: its numeric id and 4-bit metadata.
type block struct {
	id   uint16
	meta uint8
}

// section is one 16-block-high slice of a column. Blocks are packed as id<<4|meta and
// indexed y<<8|z<<4|x, the order the server sends them in.
type section [sectionVolume]uint16

// column is one chunk column of the Java world.
type column struct {
	sections [sectionCount]*section
	biomes   [biomeBytes]byte
}

// chunkPos addresses a column by chunk coordinates.
type chunkPos struct{ x, z int32 }

// world is the translator's copy of the Java world's blocks: neighbour-dependent Bedrock
// states and block interactions need the ids the server sent.
type world struct {
	columns map[chunkPos]*column
	// skyLight is whether the dimension has sky light, which decides the chunk data layout.
	skyLight bool
}

func newWorld(dimension int8) *world {
	return &world{columns: map[chunkPos]*column{}, skyLight: dimension == dimensionOverworld}
}

// errShortChunk reports chunk data shorter than its section masks announce.
var errShortChunk = errors.New("chunk data is shorter than its section masks")

// fill applies inflated chunk data to a column the way the 1.7.10 client does: block ids of
// every section in primary, then their metadata, block light and sky light, then the high id
// bits of the sections in add, then biomes when the column is sent whole. A whole column
// drops the sections it does not list.
func (w *world) fill(pos chunkPos, data []byte, primary, add uint16, whole bool) error {
	col := w.columns[pos]
	if col == nil || whole {
		col = &column{}
		w.columns[pos] = col
	}
	offset := 0
	take := func(size int) ([]byte, error) {
		if offset+size > len(data) {
			return nil, errShortChunk
		}
		chunk := data[offset : offset+size]
		offset += size
		return chunk, nil
	}
	var ids [sectionCount][]byte
	for index := range col.sections {
		if primary&(1<<index) == 0 {
			continue
		}
		low, err := take(sectionVolume)
		if err != nil {
			return err
		}
		ids[index] = low
	}
	var metadata [sectionCount][]byte
	for index := range col.sections {
		if ids[index] == nil {
			continue
		}
		nibbles, err := take(nibbleBytes)
		if err != nil {
			return err
		}
		metadata[index] = nibbles
	}
	// Block light, then sky light where the dimension has it: the client lights its own world.
	lightArrays := 1
	if w.skyLight {
		lightArrays = 2
	}
	for index := range col.sections {
		if ids[index] != nil {
			if _, err := take(nibbleBytes * lightArrays); err != nil {
				return err
			}
		}
	}
	var high [sectionCount][]byte
	for index := range col.sections {
		if add&(1<<index) == 0 {
			continue
		}
		nibbles, err := take(nibbleBytes)
		if err != nil {
			return err
		}
		high[index] = nibbles
	}
	for index := range col.sections {
		if ids[index] == nil {
			continue
		}
		filled := &section{}
		for i := range filled {
			id := uint16(ids[index][i])
			if high[index] != nil {
				id |= uint16(nibble(high[index], i)) << 8
			}
			filled[i] = id<<4 | uint16(nibble(metadata[index], i))
		}
		col.sections[index] = filled
	}
	if whole {
		biomes, err := take(biomeBytes)
		if err != nil {
			return err
		}
		copy(col.biomes[:], biomes)
	}
	return nil
}

// nibble reads the 4-bit value at index from a nibble array: even indices are the low half
// of their byte.
func nibble(data []byte, index int) uint8 {
	value := data[index>>1]
	if index&1 == 0 {
		return value & 0x0F
	}
	return value >> 4
}

// unload forgets a column.
func (w *world) unload(pos chunkPos) {
	delete(w.columns, pos)
}

// loaded reports whether the column holding block (x, z) is known.
func (w *world) loaded(x, z int32) bool {
	return w.columns[chunkPos{x >> 4, z >> 4}] != nil
}

// block returns the block at a position; unknown columns and positions outside the build
// height are air.
func (w *world) block(x, y, z int32) block {
	if y < 0 || y >= worldHeight {
		return block{}
	}
	col := w.columns[chunkPos{x >> 4, z >> 4}]
	if col == nil {
		return block{}
	}
	sec := col.sections[y>>4]
	if sec == nil {
		return block{}
	}
	packed := sec[(y&15)<<8|(z&15)<<4|(x&15)]
	return block{id: packed >> 4, meta: uint8(packed & 0x0F)}
}

// set stores a block; it reports false when the column is not loaded or y is outside the
// build height, where the 1.7.10 client ignores the change too.
func (w *world) set(x, y, z int32, value block) bool {
	if y < 0 || y >= worldHeight {
		return false
	}
	col := w.columns[chunkPos{x >> 4, z >> 4}]
	if col == nil {
		return false
	}
	sec := col.sections[y>>4]
	if sec == nil {
		if value.id == 0 {
			return true
		}
		sec = &section{}
		col.sections[y>>4] = sec
	}
	sec[(y&15)<<8|(z&15)<<4|(x&15)] = value.id<<4 | uint16(value.meta&0x0F)
	return true
}

// networkID returns the Bedrock network id for the block at a position, with the
// neighbour-dependent properties its surroundings give it.
func (w *world) networkID(x, y, z int32) uint32 {
	current := w.block(x, y, z)
	if resolved, ok := w.connected(x, y, z, current); ok {
		return resolved.hash
	}
	return legacy.BlockHash(current.id, current.meta)
}

// state returns the Bedrock state networkID identifies.
func (w *world) state(x, y, z int32) legacy.State {
	current := w.block(x, y, z)
	if resolved, ok := w.connected(x, y, z, current); ok {
		return resolved.state
	}
	return legacy.Block(current.id, current.meta)
}
