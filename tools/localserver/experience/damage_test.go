package experience

import (
	"math"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// probeDamage deals its actor 5 health points of damage that armour does not reduce.
const probeDamage = 43

const damageActor = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c"

// A result damages only its actor, once, by a finite amount of at least zero, and the damage
// names the Experience and keeps its armour choice.
func TestDamageOfHurtsOnlyTheActorOnce(t *testing.T) {
	op := func(player string, amount float64) *DamageOp {
		return &DamageOp{Player: player, Amount: amount, ReducedByArmour: true}
	}
	hit, err := damageOf("probe", op(damageActor, 4), damageActor, nil)
	if err != nil {
		t.Fatalf("damageOf refused a valid damage: %v", err)
	}
	if want := (ExperienceDamageSource{Experience: "probe", Armour: true}); hit.amount != 4 || hit.src != want {
		t.Errorf("damageOf = %+v, want 4 from %+v", hit, want)
	}
	for name, tc := range map[string]struct {
		op      *DamageOp
		actorID string
		staged  *damageHit
	}{
		"another player":  {op("00000000-0000-4000-8000-000000000001", 1), damageActor, nil},
		"no actor":        {op(damageActor, 1), "", nil},
		"second damage":   {op(damageActor, 1), damageActor, hit},
		"negative amount": {op(damageActor, -1), damageActor, nil},
		"NaN amount":      {op(damageActor, math.NaN()), damageActor, nil},
		"infinite amount": {op(damageActor, math.Inf(1)), damageActor, nil},
	} {
		if _, err := damageOf("probe", tc.op, tc.actorID, tc.staged); err == nil {
			t.Errorf("%s: damageOf accepted %+v", name, tc.op)
		}
	}
}

// Committed damage hurts a survival actor by its amount.
func TestCallbackDamagesTheActor(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeDamage)
	f.place(probeCounter, at, nil)
	f.moveTo(beside(at))
	var before float64
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		p.SetGameMode(world.GameModeSurvival)
		before = p.Health()
	})
	f.activate(at)
	f.waitTells(5*time.Second, "damage ok")
	var after float64
	f.do(func(tx *world.Tx) { after = f.player(tx).Health() })
	if before-after != 5 {
		t.Errorf("health went from %v to %v, want 5 less", before, after)
	}
}

// TestLethalDamageCommitsWithDismount keeps validated entity operations and messages intact.
func TestLethalDamageCommitsWithDismount(t *testing.T) {
	f := newEntityFixture(t)
	handle := f.spawnEntity()
	recorder := &tellRecorder{}
	f.h.tell = recorder
	f.do(f.source, func(tx *world.Tx) {
		actor, _ := f.actor.Entity(tx)
		actor.(*player.Player).SetGameMode(world.GameModeSurvival)
		vehicle, _ := f.h.findEntity(tx, "probe", handle)
		if !tx.Mount(actor, vehicle, 0) {
			t.Fatal("mount failed")
		}
	})
	f.d.seq++
	ev, snap := f.request()
	state := snap.req.Info.Context.Entities[0].State
	state.NameTag = "ejected"
	actorID := f.actor.UUID().String()
	send := &SendClientOp{Player: actorID, Channel: "probe.test", Schema: 1}
	f.do(ev.w, func(tx *world.Tx) {
		actor, _ := f.actor.Entity(tx)
		p := actor.(*player.Player)
		sends, err := f.h.apply(tx, "probe", ev, snap, []Op{
			{Entity: &EntityCommand{Action: EntityOp{Type: "dismount"}}},
			{Damage: &DamageOp{Player: actorID, Amount: p.Health()}},
			{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state}}},
			{Tell: &TellOp{Player: actorID, Text: "damage ok"}},
			{SendClient: send},
		})
		if err != nil {
			t.Fatal(err)
		}
		if !p.Dead() {
			t.Fatal("lethal damage left the actor alive")
		}
		if _, _, riding := world.Riding(p); riding {
			t.Fatal("actor is still mounted")
		}
		vehicle, _ := f.h.findEntity(tx, "probe", handle)
		if len(vehicle.snapshot().Riders) != 0 || vehicle.NameTag() != state.NameTag {
			t.Fatal("entity operations did not finish")
		}
		if len(sends) != 1 || sends[0] != send {
			t.Fatalf("client messages = %v, want the staged message", sends)
		}
	})
	if tells := recorder.all(); len(tells) != 1 || tells[0] != (recordedTell{player: actorID, text: "damage ok"}) {
		t.Fatalf("tells = %v, want the staged tell", tells)
	}
}
