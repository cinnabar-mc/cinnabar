package experience

import (
	"errors"
	"fmt"
	"math"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// SetVelocityOp gives Player, the actor, a new Velocity in blocks per tick.
type SetVelocityOp struct {
	Player   string   `json:"player"`
	Velocity Position `json:"velocity"`
}

// KnockBackOp knocks Player, the actor, back from Source by Force horizontally and Height
// upwards, in blocks per tick, as an attack's knockback does.
type KnockBackOp struct {
	Player string   `json:"player"`
	Source Position `json:"source"`
	Force  float64  `json:"force"`
	Height float64  `json:"height"`
}

const (
	typeSetVelocity = "set_velocity"
	typeKnockBack   = "knock_back"
)

type (
	setVelocityWire struct {
		variantTag
		*SetVelocityOp
	}
	knockBackWire struct {
		variantTag
		*KnockBackOp
	}
)

// actorMotion is a result's validated velocity or knockback, of which it holds at most one.
type actorMotion struct {
	velocity  *SetVelocityOp
	knockBack *KnockBackOp
}

// motionOf validates op, a set_velocity or knock_back op, for a result whose actor is actorID:
// it must move the actor, by finite values, and be the result's only motion.
func motionOf(op Op, actorID string, staged *actorMotion) (*actorMotion, error) {
	if staged != nil {
		return nil, errors.New("moves the actor a second time")
	}
	motion := &actorMotion{velocity: op.SetVelocity, knockBack: op.KnockBack}
	var target string
	var values []float64
	if op.SetVelocity != nil {
		v := op.SetVelocity.Velocity
		target, values = op.SetVelocity.Player, []float64{v.X, v.Y, v.Z}
	} else {
		k, s := op.KnockBack, op.KnockBack.Source
		target, values = k.Player, []float64{s.X, s.Y, s.Z, k.Force, k.Height}
	}
	if actorID == "" || target != actorID {
		return nil, fmt.Errorf("moves %s, who is not the actor", target)
	}
	for _, v := range values {
		if math.IsNaN(v) || math.IsInf(v, 0) {
			return nil, errors.New("moves the actor by a value that is not finite")
		}
	}
	return motion, nil
}

// apply moves actor, which commit resolved as the event's player.
func (m *actorMotion) apply(actor world.Entity) {
	p, ok := actor.(*player.Player)
	if !ok {
		return
	}
	if v := m.velocity; v != nil {
		p.SetVelocity(mgl64.Vec3{v.Velocity.X, v.Velocity.Y, v.Velocity.Z})
		return
	}
	k := m.knockBack
	p.KnockBack(mgl64.Vec3{k.Source.X, k.Source.Y, k.Source.Z}, k.Force, k.Height)
}
