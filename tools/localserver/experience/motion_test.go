package experience

import (
	"math"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

const (
	// probeVelocity sets its actor's velocity to (0.25, 0.5, -0.25).
	probeVelocity = 41
	// probeKnockBack knocks its actor back from the probe's centre, 0.4 out and 0.375 up.
	probeKnockBack = 42
)

const motionActor = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c"

// A result moves only its actor, once, and by finite values.
func TestMotionOfMovesOnlyTheActorOnce(t *testing.T) {
	velocity := func(player string, v Position) Op {
		return Op{SetVelocity: &SetVelocityOp{Player: player, Velocity: v}}
	}
	knockBack := func(force, height float64) Op {
		return Op{KnockBack: &KnockBackOp{Player: motionActor, Force: force, Height: height}}
	}
	for _, op := range []Op{velocity(motionActor, Position{0, 1, 0}), knockBack(0.4, 0.4)} {
		if _, err := motionOf(op, motionActor, nil); err != nil {
			t.Errorf("motionOf(%+v) = %v, want a motion", op, err)
		}
	}
	staged, _ := motionOf(knockBack(0.4, 0.4), motionActor, nil)
	for name, tc := range map[string]struct {
		op      Op
		actorID string
		staged  *actorMotion
	}{
		"another player":  {velocity("00000000-0000-4000-8000-000000000001", Position{}), motionActor, nil},
		"no actor":        {velocity(motionActor, Position{}), "", nil},
		"second motion":   {velocity(motionActor, Position{}), motionActor, staged},
		"NaN velocity":    {velocity(motionActor, Position{X: math.NaN()}), motionActor, nil},
		"infinite force":  {knockBack(math.Inf(1), 0.4), motionActor, nil},
		"infinite height": {knockBack(0.4, math.Inf(-1)), motionActor, nil},
		"NaN source":      {Op{KnockBack: &KnockBackOp{Player: motionActor, Source: Position{Z: math.NaN()}}}, motionActor, nil},
	} {
		if _, err := motionOf(tc.op, tc.actorID, tc.staged); err == nil {
			t.Errorf("%s: motionOf accepted %+v", name, tc.op)
		}
	}
}

// actorVelocityAfter waits for the tells to reach want, then reads the actor's velocity.
func (f *hostFixture) actorVelocityAfter(want string) mgl64.Vec3 {
	f.t.Helper()
	f.waitTells(5*time.Second, want)
	var v mgl64.Vec3
	f.do(func(tx *world.Tx) { v = f.player(tx).Velocity() })
	return v
}

// A committed velocity becomes the actor's own.
func TestCallbackSetsTheActorsVelocity(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeVelocity)
	f.place(probeCounter, at, nil)
	f.moveTo(beside(at))
	f.activate(at)
	if got, want := f.actorVelocityAfter("velocity ok"), (mgl64.Vec3{0.25, 0.5, -0.25}); !got.ApproxEqual(want) {
		t.Errorf("velocity = %v, want %v", got, want)
	}
}

// A committed knockback pushes the actor away from its source, as an attack's knockback does:
// the actor stands east of the probe, so it is pushed east and up.
func TestCallbackKnocksTheActorBack(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeKnockBack)
	f.place(probeCounter, at, nil)
	f.moveTo(beside(at))
	f.activate(at)
	if got, want := f.actorVelocityAfter("knock back ok"), (mgl64.Vec3{0.4, 0.375, 0}); !got.ApproxEqual(want) {
		t.Errorf("velocity = %v, want %v", got, want)
	}
}
