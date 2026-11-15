package translate

import (
	"math"
	"strconv"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// movementSpeedKey is the 1.7.10 player attribute that Speed, Slowness and sprinting modify.
const movementSpeedKey = "generic.movementSpeed"

// speedBaseModifierID names the modifier that carries a snapshot's base value.
const speedBaseModifierID = "base"

// maxSpeedSequence keeps snapshot sequences exact in the attribute's single-precision value.
const maxSpeedSequence = 1 << 24

// entityProperties forwards the local player's movement speed snapshot to the client, whose
// Java physics applies it as the 1.7.10 client does: every modifier, the sprint one included,
// replaced on the tick it arrives. The attribute's value is the snapshot's sequence, so a
// repeated snapshot still counts, and each modifier's name carries its amount's exact bits.
func (s *Session) entityProperties(pk *java.EntityProperties) {
	st := &s.state
	if pk.EntityID != st.entityID {
		return
	}
	for _, property := range pk.Properties {
		if property.Key != movementSpeedKey {
			continue
		}
		st.speedSequence = st.speedSequence%(maxSpeedSequence-1) + 1
		modifiers := []protocol.AttributeModifier{exactModifier(speedBaseModifierID, property.Value, 0)}
		for _, modifier := range property.Modifiers {
			modifiers = append(modifiers, exactModifier(modifier.UUID.String(), modifier.Amount, int32(modifier.Operation)))
		}
		snapshot := attribute(bridgecontract.JavaMovementAttribute, float32(st.speedSequence), 0, math.MaxFloat32)
		snapshot.Modifiers = modifiers
		s.queue(&packet.UpdateAttributes{EntityRuntimeID: runtimeID(st.entityID), Attributes: []protocol.Attribute{snapshot}})
	}
}

// exactModifier is a modifier whose name holds amount's bits in hexadecimal, as the client's
// Java physics reads it; the single-precision amount is for display only.
func exactModifier(id string, amount float64, operation int32) protocol.AttributeModifier {
	return protocol.AttributeModifier{
		ID:        id,
		Name:      strconv.FormatUint(math.Float64bits(amount), 16),
		Amount:    float32(amount),
		Operation: operation,
	}
}

// entityEffect gives the client the local player's potion effects. 1.7.10 and Bedrock number
// them alike, from Speed (1) to Saturation (23); the client's physics reads Jump Boost and
// Blindness from them, and its swings and mining Haste and Mining Fatigue.
func (s *Session) entityEffect(pk *java.EntityEffect) {
	st := &s.state
	if pk.EntityID != st.entityID {
		return
	}
	operation := byte(packet.MobEffectAdd)
	if st.effects[pk.EffectID] {
		operation = packet.MobEffectModify
	}
	st.effects[pk.EffectID] = true
	s.queue(&packet.MobEffect{
		EntityRuntimeID: runtimeID(st.entityID),
		Operation:       operation,
		EffectType:      int32(pk.EffectID),
		Amplifier:       int32(pk.Amplifier),
		Particles:       true,
		Duration:        int32(pk.Duration),
	})
}

// removeEntityEffect ends one of the local player's potion effects on the client.
func (s *Session) removeEntityEffect(pk *java.RemoveEntityEffect) {
	st := &s.state
	if pk.EntityID != st.entityID || !st.effects[int8(pk.EffectID)] {
		return
	}
	delete(st.effects, int8(pk.EffectID))
	s.queue(&packet.MobEffect{
		EntityRuntimeID: runtimeID(st.entityID),
		Operation:       packet.MobEffectRemove,
		EffectType:      int32(pk.EffectID),
	})
}

// maxPushes is how many recent explosion pushes each push event repeats, so the client keeps
// every push even when several arrive between two of its frames.
const maxPushes = 8

// push is one explosion's push of the local player, numbered in arrival order.
type push struct {
	sequence uint32
	motion   [3]float32
}

// explosion destroys the blocks an explosion breaks, as the 1.7.10 client's doExplosionB
// does, and gives the client the push it adds to the player's motion. Unlike a velocity, a
// push adds to the motion, so it travels as an event the client's Java physics applies on its
// next tick: the attribute's value is the newest push's number, and each recent push is
// three modifiers, one per axis, named by number and axis.
func (s *Session) explosion(pk *java.Explosion) {
	st := &s.state
	for _, position := range pk.AffectedBlocks() {
		s.blockChange(position.X, position.Y, position.Z, block{})
	}
	st.pushSequence = st.pushSequence%(maxSpeedSequence-1) + 1
	st.pushes = append(st.pushes, push{sequence: st.pushSequence, motion: [3]float32{pk.PlayerMotionX, pk.PlayerMotionY, pk.PlayerMotionZ}})
	if len(st.pushes) > maxPushes {
		st.pushes = st.pushes[len(st.pushes)-maxPushes:]
	}
	var modifiers []protocol.AttributeModifier
	for _, recent := range st.pushes {
		for axis, name := range [3]string{"x", "y", "z"} {
			modifiers = append(modifiers, protocol.AttributeModifier{
				ID:     strconv.FormatUint(uint64(recent.sequence), 10) + ":" + name,
				Name:   name,
				Amount: recent.motion[axis],
			})
		}
	}
	event := attribute(bridgecontract.JavaPushAttribute, float32(st.pushSequence), 0, math.MaxFloat32)
	event.Modifiers = modifiers
	s.queue(&packet.UpdateAttributes{EntityRuntimeID: runtimeID(st.entityID), Attributes: []protocol.Attribute{event}})
}
