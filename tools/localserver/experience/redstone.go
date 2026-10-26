package experience

import (
	"fmt"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// maxRedstonePower is the most redstone power a block receives or emits.
const maxRedstonePower = 15

// SetRedstonePowerOp sets the redstone power, 0 to 15, that the Experience's redstone block at
// Pos emits from every face.
type SetRedstonePowerOp struct {
	Pos   BlockPos `json:"pos"`
	Power uint8    `json:"power"`
}

const typeSetRedstonePower = "set_redstone_power"

type setRedstonePowerWire struct {
	variantTag
	*SetRedstonePowerOp
}

// powerWrite is a validated set_redstone_power op.
type powerWrite struct {
	pos   cube.Pos
	power uint8
}

// The Dragonfly redstone interfaces that Block implements. Only a redstone block takes part:
// every other one opts out, so redstone dust neither connects to it nor powers it.
var (
	_ world.RedstoneOptional    = Block{}
	_ world.RedstonePowerSource = Block{}
	_ world.RedstonePowerAction = Block{}
)

// RedstoneComponent reports whether the block takes part in redstone: only a block whose
// content rule makes it a redstone block does.
func (b Block) RedstoneComponent() bool {
	return b.t.redstone
}

// RedstonePower is the power the block emits from every face, as a block of redstone does: what
// its callbacks last set, which the hook sink keeps.
func (b Block) RedstonePower(pos cube.Pos, tx *world.Tx, _ cube.Face) int {
	return currentHooks().redstonePower(b, pos, tx)
}

// RedstonePowerAction hands a change in the power the block receives to the hook sink.
func (b Block) RedstonePowerAction(pos cube.Pos, tx *world.Tx, oldPower, newPower int) {
	if oldPower != newPower {
		currentHooks().redstoneChanged(b, pos, tx)
	}
}

// receivedPower is the redstone power, 0 to 15, that the redstone block at pos receives in tx.
func receivedPower(tx *world.Tx, pos cube.Pos) uint8 {
	return uint8(world.ClampRedstonePower(tx.RedstonePower(pos)))
}

// redstonePower is the power that the Experience's redstone block at pos emits: what the store
// keeps for it, 0 outside a world of the Host.
func (h *Host) redstonePower(b Block, pos cube.Pos, tx *world.Tx) int {
	instance, ok := h.worlds.of(tx.World())
	if !ok {
		return 0
	}
	return int(h.store.Power(b.t.exp, instance.storeKey(pos)))
}

// redstoneChanged queues on-neighbor-changed with the block's own position as the neighbor when
// the power it receives changes, admitted like any other neighbor change.
func (h *Host) redstoneChanged(b Block, pos cube.Pos, tx *world.Tx) {
	h.neighbourUpdateTick(b, pos, pos, tx)
}

// powerWriteOf validates a set_redstone_power op against the cell it writes, as the ops before
// it leave the cell: the Experience's own redstone block, and a power of at most 15.
func (h *Host) powerWriteOf(exp string, c *simCell, op *SetRedstonePowerOp) (powerWrite, error) {
	b, ok := h.reg.Lookup(c.id)
	switch {
	case !c.owned || !ok || b.t.exp != exp:
		return powerWrite{}, fmt.Errorf("sets the redstone power of %s, which is not its own block", c.id)
	case !b.t.redstone:
		return powerWrite{}, fmt.Errorf("sets the redstone power of %s, which is not a redstone block", c.id)
	case op.Power > maxRedstonePower:
		return powerWrite{}, fmt.Errorf("sets redstone power %d, at most %d", op.Power, maxRedstonePower)
	}
	return powerWrite{pos: op.Pos.cube(), power: op.Power}, nil
}

// applyPower stores what the Experience's redstone block at w.pos emits and, when that changed,
// has the redstone around it evaluated again. A later op of the result may have replaced the
// block; only its own redstone block takes the power.
func (h *Host) applyPower(tx *world.Tx, exp string, instance worldInstance, w powerWrite) {
	b, _, ok := ownBlock(tx.Block(w.pos))
	if !ok || b.t.exp != exp || !b.t.redstone {
		return
	}
	changed, err := h.store.SetPower(exp, instance.storeKey(w.pos), w.power)
	if err != nil {
		// validate checked ownership, so the store disagrees with it.
		h.log.Error("validated redstone power write failed", "experience", exp, "pos", w.pos, "error", err)
		return
	}
	if changed {
		tx.Redstone().ScheduleUpdate(w.pos)
	}
}
