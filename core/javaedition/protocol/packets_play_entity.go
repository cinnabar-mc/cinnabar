package protocol

import (
	"math"

	"github.com/google/uuid"
)

// Entity packets. Positions are fixed-point (blocks times 32), relative moves are in the same
// unit, angles are bytes where 256 steps make a full turn, and velocities are shorts in 1/8000
// block per tick.

// maxAttributeKeyChars is the longest attribute name 1.7.10 reads.
const maxAttributeKeyChars = 64

// EntityEquipment sets one equipment slot of an entity: 0 is the held item and 1 to 4 are the
// armour slots.
type EntityEquipment struct {
	playClientbound
	EntityID int32
	Slot     int16
	Item     ItemStack
}

func (*EntityEquipment) ID() int32 { return 0x04 }

func (p *EntityEquipment) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int16(p.Slot)
	w.ItemStack(p.Item)
}

func (p *EntityEquipment) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Slot = r.Int16()
	p.Item = r.ItemStack()
	return r.Err()
}

// UseBed puts a player to sleep in the bed at a block position.
type UseBed struct {
	playClientbound
	EntityID int32
	X        int32
	Y        int8
	Z        int32
}

func (*UseBed) ID() int32 { return 0x0a }

func (p *UseBed) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int32(p.X)
	w.Int8(p.Y)
	w.Int32(p.Z)
}

func (p *UseBed) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.X = r.Int32()
	p.Y = r.Int8()
	p.Z = r.Int32()
	return r.Err()
}

// Animation plays an entity animation such as an arm swing or a hurt flash.
type Animation struct {
	playClientbound
	EntityID  int32
	Animation uint8
}

func (*Animation) ID() int32 { return 0x0b }

func (p *Animation) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Uint8(p.Animation)
}

func (p *Animation) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.Animation = r.Uint8()
	return r.Err()
}

// CollectItem shows an item or experience orb flying to the entity that picked it up.
type CollectItem struct {
	playClientbound
	CollectedEntityID int32
	CollectorEntityID int32
}

func (*CollectItem) ID() int32 { return 0x0d }

func (p *CollectItem) Marshal(w *Writer) {
	w.Int32(p.CollectedEntityID)
	w.Int32(p.CollectorEntityID)
}

func (p *CollectItem) Unmarshal(r *Reader) error {
	p.CollectedEntityID = r.Int32()
	p.CollectorEntityID = r.Int32()
	return r.Err()
}

// EntityVelocity sets an entity's velocity.
type EntityVelocity struct {
	playClientbound
	EntityID  int32
	VelocityX int16
	VelocityY int16
	VelocityZ int16
}

func (*EntityVelocity) ID() int32 { return 0x12 }

func (p *EntityVelocity) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int16(p.VelocityX)
	w.Int16(p.VelocityY)
	w.Int16(p.VelocityZ)
}

func (p *EntityVelocity) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.VelocityX = r.Int16()
	p.VelocityY = r.Int16()
	p.VelocityZ = r.Int16()
	return r.Err()
}

// DestroyEntities removes entities from the world. The count is a signed byte, so one packet
// holds at most 127 ids.
type DestroyEntities struct {
	playClientbound
	EntityIDs []int32
}

func (*DestroyEntities) ID() int32 { return 0x13 }

func (p *DestroyEntities) Marshal(w *Writer) {
	w.Int8(int8(w.length(len(p.EntityIDs), math.MaxInt8, "entity ids")))
	for _, id := range p.EntityIDs {
		w.Int32(id)
	}
}

func (p *DestroyEntities) Unmarshal(r *Reader) error {
	count := r.count(int(r.Int8()), "entity ids")
	p.EntityIDs = nil
	if count > 0 {
		p.EntityIDs = make([]int32, 0, r.capacity(count, 4))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.EntityIDs = append(p.EntityIDs, r.Int32())
	}
	return r.Err()
}

// Entity names an entity without moving it.
type Entity struct {
	playClientbound
	EntityID int32
}

func (*Entity) ID() int32 { return 0x14 }

func (p *Entity) Marshal(w *Writer) {
	w.Int32(p.EntityID)
}

func (p *Entity) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	return r.Err()
}

// EntityRelativeMove moves an entity by a small fixed-point offset.
type EntityRelativeMove struct {
	playClientbound
	EntityID int32
	DX       int8
	DY       int8
	DZ       int8
}

func (*EntityRelativeMove) ID() int32 { return 0x15 }

func (p *EntityRelativeMove) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.DX)
	w.Int8(p.DY)
	w.Int8(p.DZ)
}

func (p *EntityRelativeMove) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.DX = r.Int8()
	p.DY = r.Int8()
	p.DZ = r.Int8()
	return r.Err()
}

// EntityLook turns an entity.
type EntityLook struct {
	playClientbound
	EntityID int32
	Yaw      int8
	Pitch    int8
}

func (*EntityLook) ID() int32 { return 0x16 }

func (p *EntityLook) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.Yaw)
	w.Int8(p.Pitch)
}

func (p *EntityLook) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Yaw = r.Int8()
	p.Pitch = r.Int8()
	return r.Err()
}

// EntityLookAndRelativeMove moves an entity by a small fixed-point offset and turns it.
type EntityLookAndRelativeMove struct {
	playClientbound
	EntityID int32
	DX       int8
	DY       int8
	DZ       int8
	Yaw      int8
	Pitch    int8
}

func (*EntityLookAndRelativeMove) ID() int32 { return 0x17 }

func (p *EntityLookAndRelativeMove) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.DX)
	w.Int8(p.DY)
	w.Int8(p.DZ)
	w.Int8(p.Yaw)
	w.Int8(p.Pitch)
}

func (p *EntityLookAndRelativeMove) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.DX = r.Int8()
	p.DY = r.Int8()
	p.DZ = r.Int8()
	p.Yaw = r.Int8()
	p.Pitch = r.Int8()
	return r.Err()
}

// EntityTeleport places an entity at an absolute fixed-point position.
type EntityTeleport struct {
	playClientbound
	EntityID int32
	X        int32
	Y        int32
	Z        int32
	Yaw      int8
	Pitch    int8
}

func (*EntityTeleport) ID() int32 { return 0x18 }

func (p *EntityTeleport) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int8(p.Yaw)
	w.Int8(p.Pitch)
}

func (p *EntityTeleport) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Yaw = r.Int8()
	p.Pitch = r.Int8()
	return r.Err()
}

// EntityHeadLook turns an entity's head.
type EntityHeadLook struct {
	playClientbound
	EntityID int32
	HeadYaw  int8
}

func (*EntityHeadLook) ID() int32 { return 0x19 }

func (p *EntityHeadLook) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.HeadYaw)
}

func (p *EntityHeadLook) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.HeadYaw = r.Int8()
	return r.Err()
}

// EntityStatus triggers an entity event such as a hurt or death animation.
type EntityStatus struct {
	playClientbound
	EntityID int32
	Status   int8
}

func (*EntityStatus) ID() int32 { return 0x1a }

func (p *EntityStatus) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.Status)
}

func (p *EntityStatus) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Status = r.Int8()
	return r.Err()
}

// AttachEntity mounts an entity on a vehicle or leashes it to a holder. VehicleID is -1 to
// detach.
type AttachEntity struct {
	playClientbound
	EntityID  int32
	VehicleID int32
	// Leash is 0 for riding and 1 for a leash.
	Leash uint8
}

func (*AttachEntity) ID() int32 { return 0x1b }

func (p *AttachEntity) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int32(p.VehicleID)
	w.Uint8(p.Leash)
}

func (p *AttachEntity) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.VehicleID = r.Int32()
	p.Leash = r.Uint8()
	return r.Err()
}

// EntityMetadata updates some of an entity's metadata.
type EntityMetadata struct {
	playClientbound
	EntityID int32
	Metadata Metadata
}

func (*EntityMetadata) ID() int32 { return 0x1c }

func (p *EntityMetadata) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Metadata(p.Metadata)
}

func (p *EntityMetadata) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Metadata = r.Metadata()
	return r.Err()
}

// EntityEffect applies a potion effect. Duration is in ticks.
type EntityEffect struct {
	playClientbound
	EntityID  int32
	EffectID  int8
	Amplifier int8
	Duration  int16
}

func (*EntityEffect) ID() int32 { return 0x1d }

func (p *EntityEffect) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.EffectID)
	w.Int8(p.Amplifier)
	w.Int16(p.Duration)
}

func (p *EntityEffect) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.EffectID = r.Int8()
	p.Amplifier = r.Int8()
	p.Duration = r.Int16()
	return r.Err()
}

// RemoveEntityEffect ends a potion effect.
type RemoveEntityEffect struct {
	playClientbound
	EntityID int32
	EffectID uint8
}

func (*RemoveEntityEffect) ID() int32 { return 0x1e }

func (p *RemoveEntityEffect) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Uint8(p.EffectID)
}

func (p *RemoveEntityEffect) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.EffectID = r.Uint8()
	return r.Err()
}

// AttributeModifier adjusts an attribute's base value.
type AttributeModifier struct {
	UUID      uuid.UUID
	Amount    float64
	Operation int8
}

// EntityProperty is one attribute of an entity with its modifiers.
type EntityProperty struct {
	Key       string
	Value     float64
	Modifiers []AttributeModifier
}

// EntityProperties sets attributes of an entity, such as its movement speed.
type EntityProperties struct {
	playClientbound
	EntityID   int32
	Properties []EntityProperty
}

func (*EntityProperties) ID() int32 { return 0x20 }

func (p *EntityProperties) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int32(int32(w.length(len(p.Properties), math.MaxInt32, "entity properties")))
	for _, property := range p.Properties {
		w.String(property.Key)
		w.Float64(property.Value)
		w.Int16(int16(w.length(len(property.Modifiers), math.MaxInt16, "attribute modifiers")))
		for _, modifier := range property.Modifiers {
			w.Raw(modifier.UUID[:])
			w.Float64(modifier.Amount)
			w.Int8(modifier.Operation)
		}
	}
}

// Unmarshal reads no elements for a negative count, as 1.7.10 does.
func (p *EntityProperties) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	count := int(r.Int32())
	p.Properties = nil
	if count > 0 {
		p.Properties = make([]EntityProperty, 0, r.capacity(count, 11))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		property := EntityProperty{Key: r.String(maxAttributeKeyChars), Value: r.Float64()}
		modifiers := int(r.Int16())
		if modifiers > 0 {
			property.Modifiers = make([]AttributeModifier, 0, r.capacity(modifiers, 25))
		}
		for j := 0; j < modifiers && r.Err() == nil; j++ {
			var modifier AttributeModifier
			copy(modifier.UUID[:], r.take(len(modifier.UUID), "modifier id"))
			modifier.Amount = r.Float64()
			modifier.Operation = r.Int8()
			property.Modifiers = append(property.Modifiers, modifier)
		}
		p.Properties = append(p.Properties, property)
	}
	return r.Err()
}
