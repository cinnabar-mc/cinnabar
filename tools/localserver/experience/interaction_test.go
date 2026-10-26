package experience

import (
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
)

// probePass passes its interaction's click on.
const probePass = 36

// An interaction takes the click, so the held block item is not placed, unless the guest passes
// the click on: then the item is used as on a block without an action, and the stone the player
// holds is placed on the clicked face and taken from the stack.
func TestPassedInteractionPlacesTheHeldBlock(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	stone, _ := world.ItemByName("minecraft:stone", 0)
	taken, passed := probePos(probeCount), probePos(probePass)
	f.place(probeCounter, taken, nil)
	f.place(probeCounter, passed, nil)
	holdStone := func(at BlockPos) {
		f.moveTo(beside(at))
		f.do(func(tx *world.Tx) {
			p := f.player(tx)
			p.SetGameMode(world.GameModeSurvival)
			_ = p.Inventory().SetItem(0, item.NewStack(stone, 2))
		})
	}
	above := func(at BlockPos) world.Block {
		var b world.Block
		f.do(func(tx *world.Tx) { b = tx.Block(at.cube().Side(cube.FaceUp)) })
		return b
	}
	held := func() int {
		var n int
		f.do(func(tx *world.Tx) {
			s, _ := f.player(tx).Inventory().Item(0)
			n = s.Count()
		})
		return n
	}

	holdStone(taken)
	f.activate(taken)
	f.waitTells(5*time.Second, "count 1")
	if b, ok := above(taken).(block.Air); !ok {
		t.Fatalf("a taken click placed %T above the block", b)
	}
	if n := held(); n != 2 {
		t.Fatalf("a taken click left %d stone, want 2", n)
	}

	holdStone(passed)
	f.activate(passed)
	f.waitTells(5*time.Second, "count 1", "ok")
	if b, ok := above(passed).(block.Stone); !ok {
		t.Fatalf("a passed click left %T above the block, want stone", b)
	}
	if n := held(); n != 1 {
		t.Fatalf("a passed click left %d stone, want 1", n)
	}
}
