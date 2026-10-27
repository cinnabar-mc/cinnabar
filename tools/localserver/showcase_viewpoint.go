package main

import (
	"math"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// showcaseViewpoint is where a player stands and which way they face, for the showcase
// commands that build in front of them. The facing snaps to the nearest cardinal direction so
// spawned rows and placed blocks line up with the block grid.
type showcaseViewpoint struct {
	// feet is the player's foot position; movement packets carry the eye.
	feet mgl32.Vec3
	// quarter is the facing in quarter turns from south (+Z), as Bedrock yaw counts them.
	quarter int
}

// viewpointOf reads a player's foot position and facing from their latest movement.
func viewpointOf(input *packet.PlayerAuthInput) showcaseViewpoint {
	quarter := int(math.Round(float64(input.Yaw)/90)) % 4
	if quarter < 0 {
		quarter += 4
	}
	return showcaseViewpoint{
		feet:    input.Position.Sub(mgl32.Vec3{0, actorBurstEyeHeight, 0}),
		quarter: quarter,
	}
}

// turn rotates the offset `right`, `ahead` (blocks to the side and in front, as seen facing
// south) to world X and Z for this facing.
func (v showcaseViewpoint) turn(side, ahead int) (int, int) {
	// Bedrock yaw turns clockwise seen from above: south (+Z), west (-X), north (-Z), east (+X).
	switch v.quarter {
	case 1:
		return -ahead, side
	case 2:
		return -side, -ahead
	case 3:
		return ahead, -side
	default:
		return side, ahead
	}
}

// at is the foot-level position `side` blocks across and `ahead` blocks in front of the player.
func (v showcaseViewpoint) at(side, ahead float32) mgl32.Vec3 {
	yaw := float64(v.quarter) * math.Pi / 2
	forward := mgl32.Vec3{float32(-math.Sin(yaw)), 0, float32(math.Cos(yaw))}
	across := mgl32.Vec3{float32(math.Cos(yaw)), 0, float32(math.Sin(yaw))}
	return v.feet.Add(forward.Mul(ahead)).Add(across.Mul(side))
}

// facingBack is the yaw of an actor that faces the player.
func (v showcaseViewpoint) facingBack() float32 {
	return float32(v.quarter*90 + 180)
}

// ground is the block the player stands on.
func (v showcaseViewpoint) ground() cube.Pos {
	return cube.Pos{
		int(math.Floor(float64(v.feet[0]))),
		int(math.Floor(float64(v.feet[1]))) - 1,
		int(math.Floor(float64(v.feet[2]))),
	}
}

// block is the block `side` across, `up` above and `ahead` in front of the one the player
// stands on.
func (v showcaseViewpoint) block(side, up, ahead int) cube.Pos {
	x, z := v.turn(side, ahead)
	return v.ground().Add(cube.Pos{x, up, z})
}
