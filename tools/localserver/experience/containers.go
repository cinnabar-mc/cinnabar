package experience

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/inventory"
	"github.com/df-mc/dragonfly/server/world"
)

// Containers are the positions of a callback's container reach that hold a container or are
// not loaded, in the order the snapshot found them, and whether its bounds left some out.
type Containers struct {
	Blocks    []Container `json:"blocks"`
	Truncated bool        `json:"truncated"`
}

// Container is a container block with its slots in the container's own order, and Pair, the
// other half of a double chest, whose slots these are too; or, without Loaded, a position in an
// unloaded chunk, with neither.
type Container struct {
	Pos    BlockPos     `json:"pos"`
	Loaded bool         `json:"loaded"`
	Pair   *BlockPos    `json:"pair,omitempty"`
	Slots  []*ItemStack `json:"slots,omitempty"`
}

// SetContainerSlotOp sets slot Slot of the container that the snapshot listed at Pos; a nil
// Stack empties it.
type SetContainerSlotOp struct {
	Pos   BlockPos  `json:"pos"`
	Slot  uint32    `json:"slot"`
	Stack *NewStack `json:"stack"`
}

const typeSetContainerSlot = "set_container_slot"

type setContainerSlotWire struct {
	variantTag
	*SetContainerSlotOp
}

// containerFrameReserve is the part of a callback frame that the network leaves for the
// containers: their stacks and twice MAX_CONTAINER_DATA_BYTES of hex fit it, which
// TestContainersAtTheirBoundsFitTheirReserve checks.
const containerFrameReserve = maxFrameBytes / 4

// containerBlock is a vanilla block holding a container: Dragonfly's chests, barrels, shulker
// boxes, hoppers, furnaces, blast furnaces, smokers and brewing stands.
type containerBlock interface {
	Inventory(tx *world.Tx, pos cube.Pos) *inventory.Inventory
}

// containerState is one loaded container as a snapshot holds it: its inventory and every slot's
// stack, which a commit that writes it must find unchanged.
type containerState struct {
	inv   *inventory.Inventory
	slots []item.Stack
}

// containerWrite is a validated set_container_slot op: the container's inventory, the slot and
// the stack it gets, empty to clear it.
type containerWrite struct {
	pos   cube.Pos
	inv   *inventory.Inventory
	slot  int
	stack item.Stack
}

// containers lists the containers in reach of the event's anchor and the network members snap
// holds: the anchor and each member, then their neighbours, in the network's order. Each loaded
// container is listed once, a double chest at the half found first with the other as its pair,
// and so is each unloaded position. They stop at maxContainers positions, maxContainerDataBytes
// of item data or the frame's room for them, and are then truncated.
func (h *Host) containers(tx *world.Tx, d *dispatcher, ev event, snap *snapshot) {
	if !ev.call.anchored() || len(snap.cells) == 0 {
		return
	}
	centres := []cube.Pos{ev.anchor}
	if network := snap.req.Network; network != nil {
		for _, pos := range network.Blocks {
			centres = append(centres, pos.cube())
		}
	}
	out := &Containers{Blocks: []Container{}}
	snap.req.Containers = out
	snap.containers = make(map[cube.Pos]containerState)
	req := snap.req
	req.Seq = math.MaxUint64 // Reserve the sequence number the supervisor assigns later.
	base, err := json.Marshal(Request{Callback: &req})
	if err != nil {
		out.Truncated = true
		return
	}
	remaining := maxFrameBytes - len(base)
	r := tx.Range()
	seen := make(map[cube.Pos]bool)
	data := 0
	for _, centre := range centres {
		for _, pos := range append([]cube.Pos{centre}, neighbours(centre)...) {
			if seen[pos] || pos.OutOfBounds(r) {
				continue
			}
			seen[pos] = true
			c, st, size, ok := readContainer(tx, d.id, pos)
			if !ok {
				continue
			}
			cost, _ := json.Marshal(c)
			if len(out.Blocks) > 0 {
				remaining--
			}
			if len(out.Blocks) == maxContainers || data+size > maxContainerDataBytes || len(cost) > remaining {
				out.Truncated = true
				return
			}
			remaining -= len(cost)
			data += size
			out.Blocks = append(out.Blocks, c)
			if c.Loaded {
				snap.containers[pos] = st
			}
			if c.Pair != nil {
				seen[c.Pair.cube()] = true
			}
		}
	}
}

// readContainer reads the container at pos as the Experience exp's guest sees it, its state and
// the bytes of its stacks' data; ok is false when pos is loaded and holds no container.
func readContainer(tx *world.Tx, exp string, pos cube.Pos) (c Container, st containerState, data int, ok bool) {
	c.Pos = blockPos(pos)
	b, loaded := tx.BlockLoaded(pos)
	if !loaded {
		return c, st, 0, true
	}
	holder, ok := b.(containerBlock)
	if !ok {
		return c, st, 0, false
	}
	c.Loaded = true
	st.inv = holder.Inventory(tx, pos)
	if _, chest := b.(block.Chest); chest {
		c.Pair = chestPair(tx, pos, st.inv)
	}
	st.slots = st.inv.Slots()
	c.Slots = make([]*ItemStack, len(st.slots))
	for i, s := range st.slots {
		if s.Empty() {
			continue
		}
		stack := stackOf(exp, s)
		if stack.Data != nil {
			data += hex.DecodedLen(len(*stack.Data))
		}
		c.Slots[i] = &stack
	}
	return c, st, data, true
}

// chestPair is the other half of the double chest at pos, whose inventory is inv: the loaded
// chest beside it that shares inv, or nil for a single chest.
func chestPair(tx *world.Tx, pos cube.Pos, inv *inventory.Inventory) *BlockPos {
	for _, face := range cube.HorizontalFaces() {
		side := pos.Side(face)
		if b, loaded := tx.BlockLoaded(side); loaded {
			if other, ok := b.(block.Chest); ok && other.Inventory(tx, side) == inv {
				p := blockPos(side)
				return &p
			}
		}
	}
	return nil
}

// neighbours are the six positions that share a face with pos.
func neighbours(pos cube.Pos) []cube.Pos {
	sides := make([]cube.Pos, 0, len(cube.Faces()))
	for _, face := range cube.Faces() {
		sides = append(sides, pos.Side(face))
	}
	return sides
}

// containerWrite validates op against the containers the snapshot listed: a loaded one, a slot
// it has and a stack the Experience exp may make.
func (h *Host) containerWrite(exp string, snap snapshot, op *SetContainerSlotOp) (containerWrite, error) {
	pos := op.Pos.cube()
	st, ok := snap.containers[pos]
	if !ok {
		return containerWrite{}, fmt.Errorf("it writes %v, which its snapshot lists as no loaded container", pos)
	}
	w := containerWrite{pos: pos, inv: st.inv, slot: int(op.Slot)}
	if w.slot >= len(st.slots) {
		return containerWrite{}, fmt.Errorf("it writes slot %d of the %d-slot container at %v", op.Slot, len(st.slots), pos)
	}
	if op.Stack != nil {
		var err error
		if w.stack, err = h.reg.makeStack(exp, *op.Stack); err != nil {
			return containerWrite{}, err
		}
	}
	return w, nil
}

// containersCurrent checks that every container the result writes is still the one its snapshot
// read, holding what it held then.
func containersCurrent(tx *world.Tx, snap snapshot, writes []containerWrite) error {
	checked := make(map[cube.Pos]bool, len(writes))
	for _, w := range writes {
		if checked[w.pos] {
			continue
		}
		checked[w.pos] = true
		b, loaded := tx.BlockLoaded(w.pos)
		holder, ok := b.(containerBlock)
		if !loaded || !ok || holder.Inventory(tx, w.pos) != w.inv {
			return fmt.Errorf("%w: the container at %v changed", errStale, w.pos)
		}
		now := w.inv.Slots()
		for i, was := range snap.containers[w.pos].slots {
			if !now[i].Equal(was) {
				return fmt.Errorf("%w: slot %d of the container at %v changed", errStale, i, w.pos)
			}
		}
	}
	return nil
}
