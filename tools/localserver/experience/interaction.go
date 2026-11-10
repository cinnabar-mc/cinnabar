package experience

import (
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// interaction is the click of an interaction with an Experience block, which Block.Activate
// takes while the guest decides: the clicked face, where the click met the block, from 0 to 1 on
// each axis, and the stack the player held.
type interaction struct {
	face cube.Face
	pos  mgl64.Vec3
	held item.Stack
}

// passInteraction uses the actor's held item on the block of ev, an interaction whose result
// passed its click on, as the click would have without the block's action: a block item is
// placed against the block. Nothing passes when the actor holds another stack than it clicked
// with, since the click was not meant for that one.
func passInteraction(actor world.Entity, ev event) {
	p, ok := actor.(*player.Player)
	if !ok || ev.interaction == nil {
		return
	}
	if held, _ := p.HeldItems(); !held.Equal(ev.interaction.held) {
		return
	}
	p.UseItemOnBlockWithoutActivation(ev.anchor, ev.interaction.face, ev.interaction.pos)
}
