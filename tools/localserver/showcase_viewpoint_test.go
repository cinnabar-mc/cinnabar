package main

import (
	"math"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// For each facing, the showcase entities stand at foot level four blocks in front of the
// player, facing back, and the blocks rise from the player's ground level six blocks in front.
func TestShowcasesBuildInFrontOfThePlayerAtFootLevel(t *testing.T) {
	eye := mgl32.Vec3{10.5, 64 + actorBurstEyeHeight, -3.5}
	for _, facing := range []struct {
		yaw     float32
		forward [2]float32
	}{{0, [2]float32{0, 1}}, {91, [2]float32{-1, 0}}, {-178, [2]float32{0, -1}}, {268, [2]float32{1, 0}}} {
		view := viewpointOf(&packet.PlayerAuthInput{Position: eye, Yaw: facing.yaw})
		for _, pk := range materialFxActors(view) {
			actor := pk.(*packet.AddActor)
			offset := actor.Position.Sub(view.feet)
			if math.Abs(float64(actor.Position[1]-64)) > 1e-4 {
				t.Fatalf("yaw %v: actor at height %v, not the feet", facing.yaw, actor.Position[1])
			}
			if ahead := offset[0]*facing.forward[0] + offset[2]*facing.forward[1]; math.Abs(float64(ahead-4)) > 1e-4 {
				t.Fatalf("yaw %v: actor %v blocks ahead", facing.yaw, ahead)
			}
			if back := float32(math.Mod(float64(actor.Yaw-float32(view.quarter*90)), 360)); back != 180 {
				t.Fatalf("yaw %v: actor yaw %v does not face the player", facing.yaw, actor.Yaw)
			}
		}
		ground := cube.Pos{10, 63, -4}
		for pos := range materialFxBlocks(view) {
			offset := pos.Sub(ground)
			ahead := float32(offset[0])*facing.forward[0] + float32(offset[2])*facing.forward[1]
			if ahead != 6 || offset[1] < 1 {
				t.Fatalf("yaw %v: block at %v, %v ahead", facing.yaw, offset, ahead)
			}
		}
		for _, pk := range materialShowcasePackets(view) {
			if pk, ok := pk.(*packet.AddActor); ok && math.Abs(float64(pk.Position[1]-64)) > 1e-4 {
				t.Fatalf("yaw %v: showcase actor at height %v", facing.yaw, pk.Position[1])
			}
		}
	}
}
