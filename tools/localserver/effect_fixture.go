package main

import (
	"fmt"
	"time"

	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/entity/effect"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// effectFixture gives the running player one lasting effect by network ID, for HUD and
// mob effect screen checks.
type effectFixture struct {
	ID        int               `cmd:"id"`
	Seconds   cmd.Optional[int] `cmd:"seconds"`
	Amplifier cmd.Optional[int] `cmd:"amplifier"`
}

func (f effectFixture) Run(src cmd.Source, o *cmd.Output, _ *world.Tx) {
	p, ok := src.(*player.Player)
	if !ok {
		o.Error("Only players can use this fixture")
		return
	}
	e, err := fixtureEffect(f.ID, f.Seconds.LoadOr(30), f.Amplifier.LoadOr(0))
	if err != nil {
		o.Error(err)
		return
	}
	p.AddEffect(e)
	o.Printf("Applied effect %d", f.ID)
}

// fixtureEffect builds a lasting effect with vanilla's amplifier (level minus one); a
// non-positive duration makes it infinite.
func fixtureEffect(id, seconds, amplifier int) (effect.Effect, error) {
	t, ok := effect.ByID(id)
	if !ok {
		return effect.Effect{}, fmt.Errorf("unknown effect %d", id)
	}
	lasting, ok := t.(effect.LastingType)
	if !ok {
		return effect.Effect{}, fmt.Errorf("effect %d is instant", id)
	}
	if amplifier < 0 {
		return effect.Effect{}, fmt.Errorf("amplifier %d is negative", amplifier)
	}
	if seconds <= 0 {
		return effect.NewInfinite(lasting, amplifier+1), nil
	}
	return effect.New(lasting, amplifier+1, time.Duration(seconds)*time.Second), nil
}
