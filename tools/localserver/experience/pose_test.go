package experience

import (
	"math"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// probePose tells its actor's pose.
const probePose = 40

// Angles reach the guest in the ranges the protocol promises: yaw wrapped into (-180, 180] and
// pitch clamped, so a player looking straight down is not reported as looking up.
func TestPoseAnglesStayInRange(t *testing.T) {
	for _, tc := range []struct {
		rot        cube.Rotation
		yaw, pitch float64
	}{
		{cube.Rotation{0, 0}, 0, 0},
		{cube.Rotation{-90, 30}, -90, 30},
		{cube.Rotation{180, 90}, 180, 90},
		{cube.Rotation{-180, -90}, 180, -90},
		{cube.Rotation{270, 95}, -90, 90},
		{cube.Rotation{-450, -120}, -90, -90},
		{cube.Rotation{720, 0}, 0, 0},
	} {
		yaw, pitch, ok := poseAngles(tc.rot)
		if !ok || yaw != tc.yaw || pitch != tc.pitch {
			t.Errorf("poseAngles(%v) = %v, %v, %v; want %v, %v, true", tc.rot, yaw, pitch, ok, tc.yaw, tc.pitch)
		}
	}
	for _, rot := range []cube.Rotation{{math.NaN(), 0}, {0, math.Inf(1)}, {math.Inf(-1), 0}} {
		if _, _, ok := poseAngles(rot); ok {
			t.Errorf("poseAngles(%v) accepted a rotation that is not finite", rot)
		}
	}
}

// A callback with a player actor reads where the actor faces and how it moves when it is
// snapshotted.
func TestCallbackReadsTheActorsPose(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probePose)
	f.place(probeCounter, at, nil)
	f.moveTo(beside(at))
	var want string
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		rot := p.Rotation()
		p.Move(mgl64.Vec3{}, -90-rot.Yaw(), 30-rot.Pitch())
		p.StartSneaking()
		pose := poseOf(p)
		if pose == nil {
			t.Fatal("the actor has no pose")
		}
		told := []string{"pose", "-90", "30"}
		for _, flag := range []struct {
			set  bool
			name string
		}{
			{pose.OnGround, "on_ground"},
			{pose.Sneaking, "sneaking"},
			{pose.Sprinting, "sprinting"},
			{pose.Swimming, "swimming"},
			{pose.Gliding, "gliding"},
		} {
			if flag.set {
				told = append(told, flag.name)
			}
		}
		if !pose.Sneaking {
			t.Fatal("the actor did not start sneaking")
		}
		want = strings.Join(told, " ")
	})
	f.activate(at)
	f.waitTells(5*time.Second, want)
}
