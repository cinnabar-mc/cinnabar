package protocol

// Spawn packets. Positions are fixed-point: the block coordinate times 32. Angles are bytes
// where 256 steps make a full turn. Velocities are shorts in 1/8000 block per tick.

// maxPaintingTitleChars is the longest painting title 1.7.10 reads, the length of its longest
// art name.
const maxPaintingTitleChars = len("SkullAndRoses")

// ProfileProperty is a signed property of a player's profile, such as its skin.
type ProfileProperty struct {
	Name      string
	Value     string
	Signature string
}

// SpawnPlayer adds another player to the world.
type SpawnPlayer struct {
	playClientbound
	EntityID int32
	// UUID is the player's profile id in its textual form.
	UUID       string
	Name       string
	Properties []ProfileProperty
	X, Y, Z    int32
	Yaw, Pitch int8
	// CurrentItem is the id of the held item, zero for none.
	CurrentItem int16
	Metadata    Metadata
}

func (*SpawnPlayer) ID() int32 { return 0x0c }

func (p *SpawnPlayer) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.String(p.UUID)
	w.String(p.Name)
	w.VarInt(int32(len(p.Properties)))
	for _, property := range p.Properties {
		w.String(property.Name)
		w.String(property.Value)
		w.String(property.Signature)
	}
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int8(p.Yaw)
	w.Int8(p.Pitch)
	w.Int16(p.CurrentItem)
	w.Metadata(p.Metadata)
}

// Unmarshal rejects an id that is not UUID text, because 1.7.10 parses it with java.util.UUID
// while reading. A negative property count reads no properties.
func (p *SpawnPlayer) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.UUID = r.String(maxUUIDChars)
	if r.Err() == nil {
		if err := checkUUIDText(p.UUID); err != nil {
			r.fail(err)
		}
	}
	p.Name = r.String(maxUsernameChars)
	count := int(r.VarInt())
	p.Properties = nil
	if count > 0 {
		p.Properties = make([]ProfileProperty, 0, r.capacity(count, 3))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Properties = append(p.Properties, ProfileProperty{
			Name:      r.String(MaxStringChars),
			Value:     r.String(MaxStringChars),
			Signature: r.String(MaxStringChars),
		})
	}
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Yaw = r.Int8()
	p.Pitch = r.Int8()
	p.CurrentItem = r.Int16()
	p.Metadata = r.Metadata()
	return r.Err()
}

// SpawnObject adds a vehicle, projectile or other non-living entity. Data's meaning depends on
// Type; the velocity is on the wire only when Data is positive.
type SpawnObject struct {
	playClientbound
	EntityID                        int32
	Type                            int8
	X, Y, Z                         int32
	Pitch, Yaw                      int8
	Data                            int32
	VelocityX, VelocityY, VelocityZ int16
}

func (*SpawnObject) ID() int32 { return 0x0e }

func (p *SpawnObject) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Int8(p.Type)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int8(p.Pitch)
	w.Int8(p.Yaw)
	w.Int32(p.Data)
	if p.Data > 0 {
		w.Int16(p.VelocityX)
		w.Int16(p.VelocityY)
		w.Int16(p.VelocityZ)
	}
}

func (p *SpawnObject) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.Type = r.Int8()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Pitch = r.Int8()
	p.Yaw = r.Int8()
	p.Data = r.Int32()
	p.VelocityX, p.VelocityY, p.VelocityZ = 0, 0, 0
	if p.Data > 0 {
		p.VelocityX = r.Int16()
		p.VelocityY = r.Int16()
		p.VelocityZ = r.Int16()
	}
	return r.Err()
}

// SpawnMob adds a living entity.
type SpawnMob struct {
	playClientbound
	EntityID                        int32
	Type                            uint8
	X, Y, Z                         int32
	Yaw, Pitch, HeadYaw             int8
	VelocityX, VelocityY, VelocityZ int16
	Metadata                        Metadata
}

func (*SpawnMob) ID() int32 { return 0x0f }

func (p *SpawnMob) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Uint8(p.Type)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int8(p.Yaw)
	w.Int8(p.Pitch)
	w.Int8(p.HeadYaw)
	w.Int16(p.VelocityX)
	w.Int16(p.VelocityY)
	w.Int16(p.VelocityZ)
	w.Metadata(p.Metadata)
}

func (p *SpawnMob) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.Type = r.Uint8()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Yaw = r.Int8()
	p.Pitch = r.Int8()
	p.HeadYaw = r.Int8()
	p.VelocityX = r.Int16()
	p.VelocityY = r.Int16()
	p.VelocityZ = r.Int16()
	p.Metadata = r.Metadata()
	return r.Err()
}

// SpawnPainting adds a painting. X, Y and Z are block coordinates, not fixed-point, and
// Facing is the direction the painting hangs in.
type SpawnPainting struct {
	playClientbound
	EntityID int32
	Title    string
	X, Y, Z  int32
	Facing   int32
}

func (*SpawnPainting) ID() int32 { return 0x10 }

func (p *SpawnPainting) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.String(p.Title)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int32(p.Facing)
}

func (p *SpawnPainting) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.Title = r.String(maxPaintingTitleChars)
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Facing = r.Int32()
	return r.Err()
}

// SpawnExperienceOrb adds an experience orb worth Count points.
type SpawnExperienceOrb struct {
	playClientbound
	EntityID int32
	X, Y, Z  int32
	Count    int16
}

func (*SpawnExperienceOrb) ID() int32 { return 0x11 }

func (p *SpawnExperienceOrb) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Int16(p.Count)
}

func (p *SpawnExperienceOrb) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Count = r.Int16()
	return r.Err()
}

// SpawnGlobalEntity adds an entity seen from any distance; Type 1 is a lightning bolt.
type SpawnGlobalEntity struct {
	playClientbound
	EntityID int32
	Type     int8
	X, Y, Z  int32
}

func (*SpawnGlobalEntity) ID() int32 { return 0x2c }

func (p *SpawnGlobalEntity) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Int8(p.Type)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
}

func (p *SpawnGlobalEntity) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.Type = r.Int8()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	return r.Err()
}
