package legacy

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
)

// blockFacts is a copy of crates/java-sim/fixtures/blocks.jsonl.gz, uncompressed; a test
// keeps the two identical.
//
//go:embed blockfacts.jsonl
var blockFacts string

// Facts are the per-block answers of the 1.7.10 game that neighbour rules read.
type Facts struct {
	// Name is the block's registry name.
	Name string `json:"name"`
	// Material is the name of the block's material.
	Material string `json:"material"`
	// Opaque is whether the material is opaque.
	Opaque bool `json:"opaque"`
	// RenderNormal is whether the block renders as a normal cube.
	RenderNormal bool `json:"render_normal"`
	// FullBlock is whether the block counted as a full opaque cube when it was constructed.
	FullBlock bool `json:"full_block"`
	// BlocksMovement is whether the material blocks movement.
	BlocksMovement bool `json:"blocks_movement"`
	// Solid is whether the material is solid.
	Solid bool `json:"solid"`
	// NormalCube is whether the block is an opaque, normally rendered cube that provides no
	// redstone power.
	NormalCube bool `json:"normal_cube"`
}

var loadFacts = sync.OnceValues(func() (*[MaxBlockID + 1]Facts, error) {
	var table [MaxBlockID + 1]Facts
	air := Facts{Name: "minecraft:air", Material: "air", RenderNormal: true}
	for id := range table {
		table[id] = air
	}
	for number, line := range strings.Split(blockFacts, "\n") {
		if line == "" {
			continue
		}
		var row struct {
			ID int `json:"id"`
			Facts
		}
		if err := json.Unmarshal([]byte(line), &row); err != nil {
			return nil, fmt.Errorf("blockfacts.jsonl line %d: %w", number+1, err)
		}
		if !Registered(row.ID) {
			return nil, fmt.Errorf("blockfacts.jsonl line %d: id %d is not registered", number+1, row.ID)
		}
		table[row.ID] = row.Facts
	}
	return &table, nil
})

// BlockFacts returns the facts of a block id; ids the game does not register are air.
func BlockFacts(id uint16) Facts {
	table, err := loadFacts()
	if err != nil {
		panic(err)
	}
	if id > MaxBlockID {
		return table[0]
	}
	return table[id]
}
