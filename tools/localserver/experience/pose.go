package experience

import (
	"math"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
)

// poseOf is where p faces and how it moves, as a guest reads it; nil when p's rotation is not
// finite, so a callback goes on without a pose rather than failing as malformed.
func poseOf(p *player.Player) *PlayerPose {
	yaw, pitch, ok := poseAngles(p.Rotation())
	if !ok {
		return nil
	}
	return &PlayerPose{
		Yaw:       yaw,
		Pitch:     pitch,
		OnGround:  p.OnGround(),
		Sneaking:  p.Sneaking(),
		Sprinting: p.Sprinting(),
		Swimming:  p.Swimming(),
		Gliding:   p.Gliding(),
	}
}

// poseAngles brings rot into the ranges PlayerPose promises: yaw wrapped into (-180, 180] and
// pitch clamped to [-90, 90], so looking straight down stays down. It reports false for a
// rotation that is not finite.
func poseAngles(rot cube.Rotation) (yaw, pitch float64, ok bool) {
	yaw, pitch = rot.Elem()
	if math.IsNaN(yaw) || math.IsInf(yaw, 0) || math.IsNaN(pitch) || math.IsInf(pitch, 0) {
		return 0, 0, false
	}
	yaw = math.Mod(yaw, 360)
	if yaw <= -180 {
		yaw += 360
	} else if yaw > 180 {
		yaw -= 360
	}
	return yaw, math.Max(-90, math.Min(90, pitch)), true
}
