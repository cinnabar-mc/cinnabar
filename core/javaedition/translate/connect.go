package translate

import (
	"sync"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
)

// Bedrock stores the connections of fences, panes, walls and stair corners in the block
// state, where the 1.7.10 client works them out from the neighbours when it draws the block.
// These functions apply the 1.7.10 neighbour rules and name the matching Bedrock state.

// Java block ids the connection rules name.
const (
	blockGlass            = 20
	blockFence            = 85
	blockStainedGlass     = 95
	blockIronBars         = 101
	blockGlassPane        = 102
	blockFenceGate        = 107
	blockNetherFence      = 113
	blockCobblestoneWall  = 139
	blockStainedGlassPane = 160
)

// horizontal lists the four horizontal neighbours with the Bedrock property suffix of each.
var horizontal = [4]struct {
	dx, dz int32
	name   string
}{{0, -1, "north"}, {0, 1, "south"}, {-1, 0, "west"}, {1, 0, "east"}}

// isStairs reports whether the id is one of the 1.7.10 stair blocks.
func isStairs(id uint16) bool {
	switch id {
	case 53, 67, 108, 109, 114, 128, 134, 135, 136, 156, 163, 164:
		return true
	}
	return false
}

// isPane reports whether the id is iron bars or a glass pane.
func isPane(id uint16) bool {
	return id == blockIronBars || id == blockGlassPane || id == blockStainedGlassPane
}

// connects reports whether a block's state depends on its neighbours, so a change next to it
// can change the state the client should draw.
func connects(id uint16) bool {
	return id == blockFence || id == blockNetherFence || id == blockCobblestoneWall || isPane(id) || isStairs(id)
}

// fenceConnects is the 1.7.10 fence and wall rule: the same block, a fence gate, or an
// opaque normally rendered cube that is not a gourd.
func fenceConnects(self, neighbour uint16) bool {
	if neighbour == self || neighbour == blockFenceGate {
		return true
	}
	facts := legacy.BlockFacts(neighbour)
	return facts.Opaque && facts.RenderNormal && facts.Material != "gourd"
}

// paneConnects is the 1.7.10 pane rule: a full block, glass of either kind, or any pane.
func paneConnects(neighbour uint16) bool {
	return legacy.BlockFacts(neighbour).FullBlock || neighbour == blockGlass || neighbour == blockStainedGlass || isPane(neighbour)
}

// connectedState is a cache key: a block with the connection variant its neighbours select.
type connectedState struct {
	block   block
	variant uint8
}

// resolvedState is a Bedrock state with its network hash.
type resolvedState struct {
	state legacy.State
	hash  uint32
}

var connectedStates sync.Map // connectedState -> resolvedState

// withProperties returns a block's table state with some properties replaced, caching per
// block and variant.
func withProperties(current block, variant uint8, properties func(set func(key string, value any))) resolvedState {
	key := connectedState{current, variant}
	if cached, ok := connectedStates.Load(key); ok {
		return cached.(resolvedState)
	}
	base := legacy.Block(current.id, current.meta)
	state := legacy.State{Name: base.Name, Properties: make(map[string]any, len(base.Properties))}
	for name, value := range base.Properties {
		state.Properties[name] = value
	}
	properties(func(name string, value any) {
		// Only replace properties the target state has, so a renamed property cannot
		// produce a state the client does not know.
		if _, ok := state.Properties[name]; ok {
			state.Properties[name] = value
		}
	})
	resolved := resolvedState{state: state, hash: state.NetworkHash()}
	connectedStates.Store(key, resolved)
	return resolved
}

// connected returns the state of a neighbour-dependent block with its connections, or false
// for blocks whose table state is complete.
func (w *world) connected(x, y, z int32, current block) (resolvedState, bool) {
	switch {
	case current.id == blockFence || current.id == blockNetherFence || isPane(current.id):
		var sides uint8
		for index, side := range horizontal {
			neighbour := w.block(x+side.dx, y, z+side.dz).id
			joined := false
			if isPane(current.id) {
				joined = paneConnects(neighbour)
			} else {
				joined = fenceConnects(current.id, neighbour)
			}
			if joined {
				sides |= 1 << index
			}
		}
		return withProperties(current, sides, func(set func(string, any)) {
			for index, side := range horizontal {
				set("minecraft:connection_"+side.name, sides&(1<<index) != 0)
			}
		}), true
	case current.id == blockCobblestoneWall:
		var sides uint8
		for index, side := range horizontal {
			if fenceConnects(current.id, w.block(x+side.dx, y, z+side.dz).id) {
				sides |= 1 << index
			}
		}
		return withProperties(current, sides, func(set func(string, any)) {
			for index, side := range horizontal {
				connection := "none"
				if sides&(1<<index) != 0 {
					connection = "short"
				}
				set("wall_connection_type_"+side.name, connection)
			}
			// 1.7.10 draws the post unless the wall runs straight through.
			const northSouth, westEast = 0b0011, 0b1100
			set("wall_post_bit", sides != northSouth && sides != westEast)
		}), true
	case isStairs(current.id):
		corner := w.stairCorner(x, y, z, current.meta)
		return withProperties(current, corner, func(set func(string, any)) {
			set("minecraft:corner", stairCorners[corner])
		}), true
	}
	return resolvedState{}, false
}

// stairCorners names the Bedrock corner values by variant index.
var stairCorners = [5]string{"none", "outer_left", "outer_right", "inner_left", "inner_right"}

// Stair facings by the low two metadata bits: the direction the step rises towards.
var stairFacing = [4][2]int32{{1, 0}, {-1, 0}, {0, 1}, {0, -1}}

// counterClockwise returns the stair facing a quarter turn to the left of facing.
func counterClockwise(facing uint8) uint8 {
	// east -> north -> west -> south -> east
	return [4]uint8{3, 2, 0, 1}[facing&3]
}

// stairCorner returns the index into stairCorners for a stair: an outer corner forms against
// a turned stair of the same half in front of the step and an inner corner against one
// behind it, unless the block on the corner's side is a stair with this stair's facing and
// half.
func (w *world) stairCorner(x, y, z int32, meta uint8) uint8 {
	facing, half := meta&3, meta&4
	forward := stairFacing[facing]
	turned := func(dx, dz int32) (uint8, bool) {
		neighbour := w.block(x+dx, y, z+dz)
		if !isStairs(neighbour.id) || neighbour.meta&4 != half {
			return 0, false
		}
		other := neighbour.meta & 3
		// Facings 0 and 1 run east-west, 2 and 3 north-south.
		if other>>1 == facing>>1 {
			return 0, false
		}
		return other, true
	}
	// free reports whether the block in direction dir is not a stair that continues this one.
	free := func(dir uint8) bool {
		offset := stairFacing[dir]
		neighbour := w.block(x+offset[0], y, z+offset[1])
		return !isStairs(neighbour.id) || neighbour.meta&7 != meta&7
	}
	opposite := func(dir uint8) uint8 { return dir ^ 1 }
	if other, ok := turned(forward[0], forward[1]); ok && free(opposite(other)) {
		if other == counterClockwise(facing) {
			return 1
		}
		return 2
	}
	if other, ok := turned(-forward[0], -forward[1]); ok && free(other) {
		if other == counterClockwise(facing) {
			return 3
		}
		return 4
	}
	return 0
}
