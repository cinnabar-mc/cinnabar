package legacy

import (
	_ "embed"
	"fmt"
	"strconv"
	"strings"
	"sync"
)

//go:generate go run ./internal/gen

// MaxBlockID is the highest block id Java Edition 1.7.10 registers.
const MaxBlockID = 175

// metaCount is the number of metadata values a block id has.
const metaCount = 16

//go:embed blocks.tsv
var blockTable string

// blocks holds the Bedrock state of every mapped id and metadata, loaded once.
type blocks struct {
	states [MaxBlockID + 1][metaCount]*State
	hashes [MaxBlockID + 1][metaCount]uint32
	air    uint32
}

var loadBlocks = sync.OnceValues(func() (*blocks, error) {
	table := &blocks{}
	for number, line := range strings.Split(blockTable, "\n") {
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 4 {
			return nil, fmt.Errorf("blocks.tsv line %d: %d fields", number+1, len(fields))
		}
		id, err := strconv.ParseUint(fields[0], 10, 16)
		if err != nil || id > MaxBlockID {
			return nil, fmt.Errorf("blocks.tsv line %d: invalid id %q", number+1, fields[0])
		}
		meta, err := strconv.ParseUint(fields[1], 10, 8)
		if err != nil || meta >= metaCount {
			return nil, fmt.Errorf("blocks.tsv line %d: invalid metadata %q", number+1, fields[1])
		}
		state, err := ParseState(fields[2])
		if err != nil {
			return nil, fmt.Errorf("blocks.tsv line %d: %w", number+1, err)
		}
		hash, err := strconv.ParseUint(fields[3], 10, 32)
		if err != nil || uint32(hash) != state.NetworkHash() {
			return nil, fmt.Errorf("blocks.tsv line %d: hash %q does not match %s", number+1, fields[3], state)
		}
		table.states[id][meta] = &state
		table.hashes[id][meta] = uint32(hash)
	}
	if table.states[0][0] == nil {
		return nil, fmt.Errorf("blocks.tsv has no air")
	}
	table.air = table.hashes[0][0]
	table.fillUndefined()
	return table, nil
})

// fillUndefined gives metadata the table does not list the block's lowest listed metadata,
// so every metadata of a registered id resolves to that block. Unregistered ids stay air.
func (table *blocks) fillUndefined() {
	for id := range table.states {
		first := -1
		for meta, state := range table.states[id] {
			if state != nil {
				first = meta
				break
			}
		}
		for meta := range table.states[id] {
			if table.states[id][meta] != nil {
				continue
			}
			if first < 0 {
				table.states[id][meta], table.hashes[id][meta] = table.states[0][0], table.air
			} else {
				table.states[id][meta], table.hashes[id][meta] = table.states[id][first], table.hashes[id][first]
			}
		}
	}
}

// mustBlocks returns the embedded table; a malformed table is a build defect, which the
// package's tests catch.
func mustBlocks() *blocks {
	table, err := loadBlocks()
	if err != nil {
		panic(err)
	}
	return table
}

// Block returns the Bedrock state for a 1.7.10 block id and metadata. Metadata the table does
// not list resolves to the block's lowest listed metadata; ids the game does not register are
// air, which is how its registry resolves them.
func Block(id uint16, meta uint8) State {
	table := mustBlocks()
	if id > MaxBlockID {
		return *table.states[0][0]
	}
	return *table.states[id][meta&(metaCount-1)]
}

// BlockHash returns the hashed network id of Block(id, meta).
func BlockHash(id uint16, meta uint8) uint32 {
	table := mustBlocks()
	if id > MaxBlockID {
		return table.air
	}
	return table.hashes[id][meta&(metaCount-1)]
}

// AirHash is the hashed network id of air.
func AirHash() uint32 {
	return mustBlocks().air
}

// Registered reports whether Java Edition 1.7.10 registers a block under the id: 0 to 175
// except 165 to 169, which later versions filled.
func Registered(id int) bool {
	return id >= 0 && id <= MaxBlockID && (id < 165 || id > 169)
}

// Flattened returns the flattened Java state the generator maps for an id and metadata.
// The upstream table describes Java 1.12, which gave new meanings to metadata 1.7.10 draws
// as the base block: stone variants, coarse dirt and wet sponge.
func Flattened(id, meta int, upstream string) string {
	switch {
	case id == 1 && meta != 0:
		return "minecraft:stone"
	case id == 3 && meta == 1:
		return "minecraft:dirt"
	case id == 19 && meta != 0:
		return "minecraft:sponge"
	}
	return upstream
}
