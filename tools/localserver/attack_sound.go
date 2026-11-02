package main

import (
	"net"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/sound"
	"github.com/go-gl/mathgl/mgl64"
)

// attackSounds sends a player's attack sound to every viewer except the attacker. Vanilla
// clients voice their own attack the moment they send it and Bedrock Dedicated Server sends
// the sound only to other players, while Dragonfly plays it to every viewer, so an attacker
// would hear it twice.
//
// Each World owns one instance and calls it only from its own transactions.
type attackSounds struct {
	world.NopHandler
	tx       *world.Tx
	attacker string
}

// installAttackSounds gives w its attacker-excluding attack sounds.
func installAttackSounds(w *world.World) {
	w.Handle(&attackSounds{})
}

// markAttacker records the player attacking within tx. Its attack sound plays within the same
// callback, so the mark ends with it.
func markAttacker(tx *world.Tx, attacker net.Addr) {
	sounds, ok := tx.World().Handler().(*attackSounds)
	if !ok || attacker == nil {
		return
	}
	sounds.tx, sounds.attacker = tx, attacker.String()
	tx.Defer(func(*world.Tx) { sounds.tx, sounds.attacker = nil, "" })
}

// HandleSound replays the marked attack's sound to everyone but its attacker.
func (a *attackSounds) HandleSound(ctx *world.Context, s world.Sound, pos mgl64.Vec3) {
	if _, ok := s.(sound.Attack); !ok || a.tx != ctx.Tx {
		return
	}
	attacker := a.attacker
	a.tx, a.attacker = nil, ""
	ctx.Cancel()
	for _, viewer := range othersThan(ctx.Viewers(pos), attacker) {
		viewer.ViewSound(pos, s)
	}
}

// othersThan returns the viewers whose connection address is not attacker.
func othersThan(viewers []world.Viewer, attacker string) []world.Viewer {
	others := make([]world.Viewer, 0, len(viewers))
	for _, viewer := range viewers {
		if addressed, ok := viewer.(interface{ Addr() net.Addr }); ok {
			if addr := addressed.Addr(); addr != nil && addr.String() == attacker {
				continue
			}
		}
		others = append(others, viewer)
	}
	return others
}
