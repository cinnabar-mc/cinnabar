package translate

import (
	"encoding/binary"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Bedrock dimension ids.
const (
	bedrockOverworld = 0
	bedrockNether    = 1
	bedrockEnd       = 2
)

// subChunkVersion is the Bedrock sub-chunk format that carries its own Y index.
const subChunkVersion = 9

// dimensionLayout describes how a Java dimension's 16 sections sit in the Bedrock dimension
// the client draws them in.
type dimensionLayout struct {
	// bedrock is the Bedrock dimension id.
	bedrock int32
	// below is the number of Bedrock sub-chunks under Java section 0.
	below int
	// sections is how many Java sections the Bedrock dimension has room for.
	sections int
	// total is the Bedrock dimension's sub-chunk count, which is also its biome slot count.
	total int
}

// layoutOf returns the layout for a Java dimension. The Bedrock overworld starts 64 blocks
// lower, and the Bedrock nether is 128 blocks high: Java nether blocks above that are not
// shown.
func layoutOf(dimension int8) dimensionLayout {
	switch dimension {
	case dimensionNether:
		return dimensionLayout{bedrock: bedrockNether, sections: 8, total: 8}
	case dimensionEnd:
		return dimensionLayout{bedrock: bedrockEnd, sections: sectionCount, total: 16}
	default:
		return dimensionLayout{bedrock: bedrockOverworld, below: 4, sections: sectionCount, total: 24}
	}
}

// levelChunk builds the Bedrock LevelChunk packet for a loaded column.
func (w *world) levelChunk(pos chunkPos, dimension int8) *packet.LevelChunk {
	col := w.columns[pos]
	layout := layoutOf(dimension)
	top := 0
	for index := 0; index < layout.sections; index++ {
		if col.sections[index] != nil {
			top = index + 1
		}
	}
	var payload []byte
	for index := -layout.below; index < top; index++ {
		payload = append(payload, subChunkVersion)
		if index < 0 || col.sections[index] == nil {
			payload = append(payload, 0, byte(int8(index)))
			continue
		}
		payload = append(payload, 1, byte(int8(index)))
		payload = w.appendBlockStorage(payload, pos, index, col.sections[index])
	}
	payload = appendBiomes(payload, &col.biomes, layout.total)
	// No border blocks and no block entities.
	payload = append(payload, 0)
	return &packet.LevelChunk{
		Position:      protocol.ChunkPos{pos.x, pos.z},
		Dimension:     layout.bedrock,
		SubChunkCount: uint32(layout.below + top),
		RawPayload:    payload,
	}
}

// appendBlockStorage appends one section as a Bedrock paletted storage of network ids,
// indexed x<<8|z<<4|y.
func (w *world) appendBlockStorage(payload []byte, pos chunkPos, index int, sec *section) []byte {
	palette := make([]uint32, 0, 16)
	lookup := make(map[uint32]uint16, 16)
	var indices [sectionVolume]uint16
	// Blocks that do not depend on their neighbours share one id per packed block value.
	plain := make(map[uint16]uint16, 16)
	baseX, baseY, baseZ := pos.x<<4, int32(index)<<4, pos.z<<4
	for x := int32(0); x < 16; x++ {
		for z := int32(0); z < 16; z++ {
			for y := int32(0); y < 16; y++ {
				packed := sec[y<<8|z<<4|x]
				slot, known := plain[packed]
				if !known {
					id := w.networkID(baseX+x, baseY+y, baseZ+z)
					var listed bool
					if slot, listed = lookup[id]; !listed {
						slot = uint16(len(palette))
						palette = append(palette, id)
						lookup[id] = slot
					}
					if !connects(packed >> 4) {
						plain[packed] = slot
					}
				}
				indices[x<<8|z<<4|y] = slot
			}
		}
	}
	return appendPaletted(payload, indices[:], palette)
}

// paletteBits lists the index widths a Bedrock paletted storage may use.
var paletteBits = [...]int{1, 2, 3, 4, 5, 6, 8, 16}

// appendPaletted appends a network paletted storage: a header naming the index width, the
// indices packed into 32-bit words, then the palette. A one-entry palette needs no indices.
func appendPaletted(payload []byte, indices []uint16, palette []uint32) []byte {
	if len(palette) == 1 {
		payload = append(payload, 1)
		return appendVarInt32(payload, int32(palette[0]))
	}
	bits := 16
	for _, candidate := range paletteBits {
		if len(palette) <= 1<<candidate {
			bits = candidate
			break
		}
	}
	payload = append(payload, byte(bits<<1|1))
	perWord := 32 / bits
	for start := 0; start < len(indices); start += perWord {
		var word uint32
		for offset := 0; offset < perWord && start+offset < len(indices); offset++ {
			word |= uint32(indices[start+offset]) << (offset * bits)
		}
		payload = binary.LittleEndian.AppendUint32(payload, word)
	}
	payload = appendVarInt32(payload, int32(len(palette)))
	for _, id := range palette {
		payload = appendVarInt32(payload, int32(id))
	}
	return payload
}

// appendBiomes appends the column's biomes. Java 1.7.10 biomes do not vary with height, so
// the lowest slot holds them and the remaining slots repeat it.
func appendBiomes(payload []byte, biomes *[biomeBytes]byte, slots int) []byte {
	palette := make([]uint32, 0, 4)
	lookup := make(map[byte]uint16, 4)
	var indices [sectionVolume]uint16
	for x := 0; x < 16; x++ {
		for z := 0; z < 16; z++ {
			biome := biomes[z<<4|x]
			slot, known := lookup[biome]
			if !known {
				slot = uint16(len(palette))
				palette = append(palette, uint32(bedrockBiome(biome)))
				lookup[biome] = slot
			}
			for y := 0; y < 16; y++ {
				indices[x<<8|z<<4|y] = slot
			}
		}
	}
	payload = appendPaletted(payload, indices[:], palette)
	for slot := 1; slot < slots; slot++ {
		// A storage that copies the one below it.
		payload = append(payload, 0xFF)
	}
	return payload
}

// appendVarInt32 appends a zigzag-encoded signed varint.
func appendVarInt32(payload []byte, value int32) []byte {
	encoded := uint32(value<<1) ^ uint32(value>>31)
	for encoded >= 0x80 {
		payload = append(payload, byte(encoded)|0x80)
		encoded >>= 7
	}
	return append(payload, byte(encoded))
}

// bedrockBiome returns the Bedrock biome id for a 1.7.10 biome id. Ids pass through
// unchanged; whether each number names the same biome in both editions is not yet checked.
func bedrockBiome(biome byte) int32 {
	return int32(biome)
}
