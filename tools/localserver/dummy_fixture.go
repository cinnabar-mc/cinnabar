package main

import (
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// dummyHealth keeps the target standing through a long run of unarmed and critical hits.
const dummyHealth = 1000

// targetDummy spawns a sessionless player two blocks ahead of the caller, facing it, so local
// melee hits reach a real server-side target.
type targetDummy struct{}

// dummyHandler marks a target dummy; attacks on it carry no knockback, so it stays in reach.
type dummyHandler struct{ player.NopHandler }

// isTargetDummy reports whether e is a dummy spawned by fixture_dummy.
func isTargetDummy(e world.Entity) bool {
	p, ok := e.(*player.Player)
	if !ok {
		return false
	}
	_, ok = p.Handler().(dummyHandler)
	return ok
}

// Run spawns the dummy in the caller's world.
func (targetDummy) Run(src cmd.Source, o *cmd.Output, tx *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("only players can spawn a target dummy")
		return
	}
	facing := cube.Rotation{p.Rotation().Yaw(), 0}
	pos := p.Position().Add(facing.Vec3().Mul(2))
	dummy := tx.AddEntity(world.EntitySpawnOpts{Position: pos}.New(player.Type, player.Config{
		Name:      "Target",
		Position:  pos,
		Rotation:  facing.Opposite(),
		Health:    dummyHealth,
		MaxHealth: dummyHealth,
	})).(*player.Player)
	dummy.Handle(dummyHandler{})
	o.Print("Spawned a target dummy two blocks ahead")
}
