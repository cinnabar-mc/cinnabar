package experience

import (
	"errors"
	"fmt"
	"math"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// DamageOp deals Amount health points of damage to Player, the actor, reduced by armour when
// ReducedByArmour is set.
type DamageOp struct {
	Player          string  `json:"player"`
	Amount          float64 `json:"amount"`
	ReducedByArmour bool    `json:"reduced_by_armour"`
}

const typeDamage = "damage"

type damageWire struct {
	variantTag
	*DamageOp
}

// ExperienceDamageSource is damage an Experience deals. Like generic vanilla damage it is
// reduced by the resistance effect, is not fire and leaves a held totem to save the player;
// whether armour reduces it is the Experience's choice.
type ExperienceDamageSource struct {
	// Experience is the id of the Experience that dealt the damage.
	Experience string
	Armour     bool
}

func (s ExperienceDamageSource) ReducedByArmour() bool   { return s.Armour }
func (ExperienceDamageSource) ReducedByResistance() bool { return true }
func (ExperienceDamageSource) Fire() bool                { return false }
func (ExperienceDamageSource) IgnoreTotem() bool         { return false }

// damageOf validates op, a damage op of the Experience exp, for a result whose actor is actorID:
// it must hurt the actor, by a finite amount of at least zero, and be the result's only damage.
func damageOf(exp string, op *DamageOp, actorID string, staged *damageHit) (*damageHit, error) {
	switch {
	case staged != nil:
		return nil, errors.New("damages the actor a second time")
	case actorID == "" || op.Player != actorID:
		return nil, fmt.Errorf("damages %s, who is not the actor", op.Player)
	case math.IsNaN(op.Amount) || math.IsInf(op.Amount, 0) || op.Amount < 0:
		return nil, fmt.Errorf("damages the actor by %v, not a finite amount of at least zero", op.Amount)
	}
	return &damageHit{amount: op.Amount, src: ExperienceDamageSource{Experience: exp, Armour: op.ReducedByArmour}}, nil
}

// damageHit is a result's validated damage op.
type damageHit struct {
	amount float64
	src    ExperienceDamageSource
}

// apply hurts actor, which commit resolved as the event's player, as vanilla damage does.
func (d *damageHit) apply(actor world.Entity) {
	if p, ok := actor.(*player.Player); ok {
		p.Hurt(d.amount, d.src)
	}
}
