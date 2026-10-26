package experience

import (
	"fmt"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// probeRedstone tells the power its block receives, then has it emit 15.
const probeRedstone = 38

// A redstone block reads the power a vanilla block of redstone beside it gives, and the power it
// then emits lights the redstone dust on its other side. A block whose rule leaves it out of
// redstone can do neither.
func TestRedstoneBlockReadsPowerAndLightsDust(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeRedstone)
	f.place(probeNode, at, nil)
	source, dust := at.cube().Side(cube.FaceWest), at.cube().Side(cube.FaceSouth)
	dustPower := func() int {
		var power int
		f.do(func(tx *world.Tx) {
			if wire, ok := tx.Block(dust).(block.RedstoneWire); ok {
				power = wire.Power
			}
		})
		return power
	}
	f.do(func(tx *world.Tx) {
		tx.SetBlock(source, block.RedstoneBlock{}, nil)
		tx.SetBlock(dust.Side(cube.FaceDown), block.Stone{}, nil)
		tx.SetBlock(dust, block.RedstoneWire{}, nil)
	})
	if power := dustPower(); power != 0 {
		t.Fatalf("dust beside an emitter that emits nothing has power %d", power)
	}
	f.moveTo(beside(at))
	f.activate(at)
	f.waitTells(5*time.Second, "power 15 emit ok")
	// A world ticks, and so evaluates redstone, only while something views it. The viewer loads
	// only the emitter's chunk, which is loaded already, so no later snapshot sees a chunk load.
	f.do(func(tx *world.Tx) {
		viewer := world.NewLoader(0, tx.World(), world.NopViewer{})
		viewer.Move(tx, at.cube().Vec3Centre())
		viewer.Load(tx, 1)
	})
	f.waitUntil(5*time.Second, func() bool { return dustPower() == maxRedstonePower }, func() string {
		return fmt.Sprintf("dust beside the emitter has power %d, want %d", dustPower(), maxRedstonePower)
	})
	if tok, ok := f.store.Token("probe", overworldKey(at)); !ok || f.store.Power("probe", overworldKey(at)) != maxRedstonePower {
		t.Fatalf("the store keeps power %d for %+v, want %d", f.store.Power("probe", overworldKey(at)), tok, maxRedstonePower)
	}

	plain := probePos(probeRedstone)
	plain.Z += 4
	f.place(probeCounter, plain, nil)
	f.moveTo(beside(plain))
	f.activate(plain)
	f.waitTells(5*time.Second, "power 15 emit ok", "power unsupported-state emit unsupported-state")
}
