package main

import (
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// The gallery's layout, in blocks across (negative to the player's right), up from the ground
// and ahead of the player:
//
//   - the five mobs in a row four blocks ahead;
//   - the arena floor, 9 wide and 7 deep, from six blocks ahead, in the ground itself;
//   - a black hole of 3×3×3 blocks floating over the arena;
//   - the ship in a bottle and the snow globe on the right, the lava lamp on the left, and 2×2
//     clusters of breathing diamond (right) and redstone (left) ore;
//   - a corruptible stone wall 15 blocks ahead, 17 wide and 5 high, holding the hollow
//     aquarium and mineshaft, two carved runes, the 2×2 living painting, the 2×3 star portal
//     and the 4×3 screen.
const (
	materialFxWallAhead  = 15
	materialFxArenaAhead = 9
)

// materialFxSpan lists the offsets `low..=high`.
func materialFxSpan(low, high int) []int {
	span := make([]int, 0, high-low+1)
	for value := low; value <= high; value++ {
		span = append(span, value)
	}
	return span
}

// materialFxBlocks are the gallery's block placements in front of `view`.
func materialFxBlocks(view showcaseViewpoint) map[cube.Pos]string {
	placed := map[cube.Pos]string{}
	put := func(name string, sides, ups, aheads []int) {
		for _, side := range sides {
			for _, up := range ups {
				for _, ahead := range aheads {
					placed[view.block(side, up, ahead)] = materialFxID + ":" + name
				}
			}
		}
	}
	one := func(value int) []int { return []int{value} }
	put("arena", materialFxSpan(-4, 4), one(0), materialFxSpan(6, 12))
	put("blackhole", materialFxSpan(-1, 1), materialFxSpan(3, 5), materialFxSpan(8, 10))
	put("bottle", one(-6), one(1), one(7))
	put("snowglobe", one(-6), one(1), one(9))
	put("lavalamp", one(6), one(1), one(7))
	put("glintore", materialFxSpan(-7, -6), materialFxSpan(1, 2), one(11))
	put("pulseore", materialFxSpan(6, 7), materialFxSpan(1, 2), one(11))
	wall := one(materialFxWallAhead)
	put("wallstone", materialFxSpan(-8, 8), materialFxSpan(1, 5), wall)
	put("aquarium", one(-7), one(2), wall)
	put("mineshaft", one(-5), one(2), wall)
	put("rune", []int{-7, -5}, one(4), wall)
	put("canvas", materialFxSpan(-3, -2), materialFxSpan(2, 3), wall)
	put("vortex", materialFxSpan(0, 1), materialFxSpan(1, 3), wall)
	put("screen", materialFxSpan(3, 6), materialFxSpan(2, 4), wall)
	return placed
}

// materialFxCentre is the centre of the block cluster spanning the given offsets.
func materialFxCentre(view showcaseViewpoint, sides, ups, aheads [2]int) mgl32.Vec3 {
	low, high := view.block(sides[0], ups[0], aheads[0]), view.block(sides[1], ups[1], aheads[1])
	return mgl32.Vec3{
		float32(low[0]+high[0])/2 + 0.5,
		float32(low[1]+high[1])/2 + 0.5,
		float32(low[2]+high[2])/2 + 0.5,
	}
}

// materialFxPieces are the centres the client part needs, by piece index.
func materialFxPieces(view showcaseViewpoint) [5]mgl32.Vec3 {
	wall := [2]int{materialFxWallAhead, materialFxWallAhead}
	arena := materialFxCentre(view, [2]int{0, 0}, [2]int{0, 0}, [2]int{materialFxArenaAhead, materialFxArenaAhead})
	return [5]mgl32.Vec3{
		materialFxArenaPiece:     arena.Add(mgl32.Vec3{0, 0.5, 0}),
		materialFxBlackHolePiece: materialFxCentre(view, [2]int{-1, 1}, [2]int{3, 5}, [2]int{8, 10}),
		materialFxPaintingPiece:  materialFxCentre(view, [2]int{-3, -2}, [2]int{2, 3}, wall),
		materialFxScreenPiece:    materialFxCentre(view, [2]int{3, 6}, [2]int{2, 4}, wall),
		materialFxPortalPiece:    materialFxCentre(view, [2]int{0, 1}, [2]int{1, 3}, wall),
	}
}

// materialFxCorruptionSource is where the corruption starts: the wall's end on the player's
// right, a block above the ground.
func materialFxCorruptionSource(view showcaseViewpoint) mgl32.Vec3 {
	return materialFxCentre(view, [2]int{-8, -8}, [2]int{3, 3}, [2]int{materialFxWallAhead, materialFxWallAhead})
}

// materialFxLayoutRecords tell the client part where the gallery's pieces stand and which way
// it faces.
func materialFxLayoutRecords(view showcaseViewpoint) [][]cinnabarext.Scalar {
	records := [][]cinnabarext.Scalar{materialFxRecord(materialFxFacing, int64(view.quarter))}
	for piece, centre := range materialFxPieces(view) {
		records = append(records, materialFxRecord(materialFxLayout, int64(piece), milli(centre[0]), milli(centre[1]), milli(centre[2])))
	}
	return records
}

// materialFxMobFeet is where the showcase mob at index stands: at foot level in a row four blocks in
// front of the player, two blocks apart and centred on them.
func materialFxMobFeet(view showcaseViewpoint, index int) mgl32.Vec3 {
	return view.at(float32(index*2-(len(materialFxEntities)-1)), 4)
}

// materialFxActors spawns the showcase entities at foot level in a row four blocks in front of
// the player, facing them.
func materialFxActors(view showcaseViewpoint) []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialFxEntities))
	for index, entity := range materialFxEntities {
		id := int64(materialFxFirstID + index)
		packets = append(packets, &packet.AddActor{
			EntityUniqueID: id, EntityRuntimeID: uint64(id), EntityType: entity.id,
			Position: materialFxMobFeet(view, index),
			Yaw:      view.facingBack(), HeadYaw: view.facingBack(), BodyYaw: view.facingBack(),
			EntityMetadata: actorBurstMetadata(),
		})
	}
	return packets
}

// materialFxClear removes the previously spawned entities.
func materialFxClear() []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialFxEntities))
	for index := range materialFxEntities {
		packets = append(packets, &packet.RemoveActor{EntityUniqueID: int64(materialFxFirstID + index)})
	}
	return packets
}
