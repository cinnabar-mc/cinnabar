package protocol

import (
	"encoding/binary"
	"math"
)

// World packets. Block positions are plain block coordinates unless a field says otherwise.

// Field length limits, in UTF-16 units, that 1.7.10 enforces while reading.
const (
	maxSignLineChars     = 15
	maxSoundNameChars    = 256
	maxParticleNameChars = 64
)

// blockActionIDMask keeps the bits of a block id that BlockAction carries.
const blockActionIDMask = 0xfff

// SpawnPosition sets the world spawn point the compass points at.
type SpawnPosition struct {
	playClientbound
	X int32
	Y int32
	Z int32
}

func (*SpawnPosition) ID() int32 { return 0x05 }

func (p *SpawnPosition) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
}

func (p *SpawnPosition) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	return r.Err()
}

// TimeUpdate sets the world clock. A negative TimeOfDay means the daylight cycle is stopped.
type TimeUpdate struct {
	playClientbound
	WorldAge  int64
	TimeOfDay int64
}

func (*TimeUpdate) ID() int32 { return 0x03 }

func (p *TimeUpdate) Marshal(w *Writer) {
	w.Int64(p.WorldAge)
	w.Int64(p.TimeOfDay)
}

func (p *TimeUpdate) Unmarshal(r *Reader) error {
	p.WorldAge = r.Int64()
	p.TimeOfDay = r.Int64()
	return r.Err()
}

// BlockChange replaces one block.
type BlockChange struct {
	playClientbound
	X        int32
	Y        uint8
	Z        int32
	BlockID  int32
	Metadata uint8
}

func (*BlockChange) ID() int32 { return 0x23 }

func (p *BlockChange) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Uint8(p.Y)
	w.Int32(p.Z)
	w.VarInt(p.BlockID)
	w.Uint8(p.Metadata)
}

func (p *BlockChange) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Uint8()
	p.Z = r.Int32()
	p.BlockID = r.VarInt()
	p.Metadata = r.Uint8()
	return r.Err()
}

// MultiBlockChange replaces several blocks of one chunk column. Data holds four bytes per
// record; Records decodes them.
type MultiBlockChange struct {
	playClientbound
	ChunkX, ChunkZ int32
	RecordCount    uint16
	Data           []byte
}

// BlockChangeRecord is one decoded MultiBlockChange entry. X and Z are positions within the
// chunk column, 0 to 15.
type BlockChangeRecord struct {
	X, Z     uint8
	Y        uint8
	BlockID  uint16
	Metadata uint8
}

// NewMultiBlockChange builds the packet for records, packing each as 1.7.10 does. Block ids
// are cut to 12 bits and metadata, X and Z to 4. The count is 16 bits on the wire, so
// records past 65,535 are left out.
func NewMultiBlockChange(chunkX, chunkZ int32, records []BlockChangeRecord) *MultiBlockChange {
	records = records[:min(len(records), math.MaxUint16)]
	p := &MultiBlockChange{ChunkX: chunkX, ChunkZ: chunkZ, RecordCount: uint16(len(records))}
	for _, record := range records {
		p.Data = binary.BigEndian.AppendUint16(p.Data, uint16(record.X&0xf)<<12|uint16(record.Z&0xf)<<8|uint16(record.Y))
		p.Data = binary.BigEndian.AppendUint16(p.Data, record.BlockID&0xfff<<4|uint16(record.Metadata&0xf))
	}
	return p
}

// Records decodes Data the way the client applies it: RecordCount entries, stopping without
// an error where Data runs out.
func (p *MultiBlockChange) Records() []BlockChangeRecord {
	count := min(int(p.RecordCount), len(p.Data)/4)
	records := make([]BlockChangeRecord, 0, count)
	for i := 0; i < count; i++ {
		position := binary.BigEndian.Uint16(p.Data[4*i:])
		block := binary.BigEndian.Uint16(p.Data[4*i+2:])
		records = append(records, BlockChangeRecord{
			X:        uint8(position >> 12 & 0xf),
			Z:        uint8(position >> 8 & 0xf),
			Y:        uint8(position),
			BlockID:  block >> 4,
			Metadata: uint8(block & 0xf),
		})
	}
	return records
}

func (*MultiBlockChange) ID() int32 { return 0x22 }

func (p *MultiBlockChange) Marshal(w *Writer) {
	w.Int32(p.ChunkX)
	w.Int32(p.ChunkZ)
	w.Uint16(p.RecordCount)
	w.Int32(int32(w.length(len(p.Data), math.MaxInt32, "record bytes")))
	w.Raw(p.Data)
}

// Unmarshal leaves Data empty for a data length of zero or less.
func (p *MultiBlockChange) Unmarshal(r *Reader) error {
	p.ChunkX = r.Int32()
	p.ChunkZ = r.Int32()
	p.RecordCount = r.Uint16()
	p.Data = nil
	if size := r.Int32(); size > 0 {
		p.Data = r.Bytes(int(size))
	}
	return r.Err()
}

// BlockAction triggers a block event such as a piston move, a chest lid or a note. The block
// id is cut to its low 12 bits on the wire.
type BlockAction struct {
	playClientbound
	X            int32
	Y            int16
	Z            int32
	Data1, Data2 uint8
	BlockID      int32
}

func (*BlockAction) ID() int32 { return 0x24 }

func (p *BlockAction) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int16(p.Y)
	w.Int32(p.Z)
	w.Uint8(p.Data1)
	w.Uint8(p.Data2)
	w.VarInt(p.BlockID & blockActionIDMask)
}

func (p *BlockAction) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int16()
	p.Z = r.Int32()
	p.Data1 = r.Uint8()
	p.Data2 = r.Uint8()
	p.BlockID = r.VarInt() & blockActionIDMask
	return r.Err()
}

// BlockBreakAnimation shows the cracks of a block another entity is breaking. EntityID
// identifies the breaker.
type BlockBreakAnimation struct {
	playClientbound
	EntityID int32
	X        int32
	Y        int32
	Z        int32
	Stage    uint8
}

func (*BlockBreakAnimation) ID() int32 { return 0x25 }

func (p *BlockBreakAnimation) Marshal(w *Writer) {
	w.VarInt(p.EntityID)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Uint8(p.Stage)
}

func (p *BlockBreakAnimation) Unmarshal(r *Reader) error {
	p.EntityID = r.VarInt()
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Stage = r.Uint8()
	return r.Err()
}

// ExplosionRecord is the offset of one destroyed block from the explosion's centre.
type ExplosionRecord struct {
	DX, DY, DZ int8
}

// Explosion plays an explosion, removes the affected blocks and pushes the player.
type Explosion struct {
	playClientbound
	X, Y, Z                                     float32
	Radius                                      float32
	Records                                     []ExplosionRecord
	PlayerMotionX, PlayerMotionY, PlayerMotionZ float32
}

func (*Explosion) ID() int32 { return 0x27 }

// AffectedBlocks returns the absolute block positions of Records. The client adds each offset
// to the centre truncated toward zero.
func (p *Explosion) AffectedBlocks() []Position {
	x, y, z := truncateToInt32(p.X), truncateToInt32(p.Y), truncateToInt32(p.Z)
	blocks := make([]Position, 0, len(p.Records))
	for _, record := range p.Records {
		blocks = append(blocks, Position{X: x + int32(record.DX), Y: y + int32(record.DY), Z: z + int32(record.DZ)})
	}
	return blocks
}

// truncateToInt32 converts like a Java float-to-int cast: toward zero, saturating at the
// int range, with NaN as zero.
func truncateToInt32(f float32) int32 {
	switch {
	case f != f:
		return 0
	case f >= math.MaxInt32:
		return math.MaxInt32
	case f <= math.MinInt32:
		return math.MinInt32
	}
	return int32(f)
}

func (p *Explosion) Marshal(w *Writer) {
	w.Float32(p.X)
	w.Float32(p.Y)
	w.Float32(p.Z)
	w.Float32(p.Radius)
	w.Int32(int32(w.length(len(p.Records), math.MaxInt32, "explosion records")))
	for _, record := range p.Records {
		w.Int8(record.DX)
		w.Int8(record.DY)
		w.Int8(record.DZ)
	}
	w.Float32(p.PlayerMotionX)
	w.Float32(p.PlayerMotionY)
	w.Float32(p.PlayerMotionZ)
}

func (p *Explosion) Unmarshal(r *Reader) error {
	p.X = r.Float32()
	p.Y = r.Float32()
	p.Z = r.Float32()
	p.Radius = r.Float32()
	count := r.count(int(r.Int32()), "explosion records")
	p.Records = nil
	if count > 0 {
		p.Records = make([]ExplosionRecord, 0, r.capacity(count, 3))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Records = append(p.Records, ExplosionRecord{DX: r.Int8(), DY: r.Int8(), DZ: r.Int8()})
	}
	p.PlayerMotionX = r.Float32()
	p.PlayerMotionY = r.Float32()
	p.PlayerMotionZ = r.Float32()
	return r.Err()
}

// Effect plays a world sound or particle effect at a block. Global effects ignore distance.
type Effect struct {
	playClientbound
	EffectID int32
	X        int32
	Y        uint8
	Z        int32
	Data     int32
	Global   bool
}

func (*Effect) ID() int32 { return 0x28 }

func (p *Effect) Marshal(w *Writer) {
	w.Int32(p.EffectID)
	w.Int32(p.X)
	w.Uint8(p.Y)
	w.Int32(p.Z)
	w.Int32(p.Data)
	w.Bool(p.Global)
}

func (p *Effect) Unmarshal(r *Reader) error {
	p.EffectID = r.Int32()
	p.X = r.Int32()
	p.Y = r.Uint8()
	p.Z = r.Int32()
	p.Data = r.Int32()
	p.Global = r.Bool()
	return r.Err()
}

// SoundEffect plays a named sound. The position is fixed-point with 8 units per block and Pitch
// is 63 at normal speed.
type SoundEffect struct {
	playClientbound
	Name   string
	X      int32
	Y      int32
	Z      int32
	Volume float32
	Pitch  uint8
}

func (*SoundEffect) ID() int32 { return 0x29 }

func (p *SoundEffect) Marshal(w *Writer) {
	w.String(p.Name)
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
	w.Float32(p.Volume)
	w.Uint8(p.Pitch)
}

func (p *SoundEffect) Unmarshal(r *Reader) error {
	p.Name = r.String(maxSoundNameChars)
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	p.Volume = r.Float32()
	p.Pitch = r.Uint8()
	return r.Err()
}

// Particle spawns Count particles spread by the offsets around a position in blocks.
type Particle struct {
	playClientbound
	Name    string
	X       float32
	Y       float32
	Z       float32
	OffsetX float32
	OffsetY float32
	OffsetZ float32
	Speed   float32
	Count   int32
}

func (*Particle) ID() int32 { return 0x2a }

func (p *Particle) Marshal(w *Writer) {
	w.String(p.Name)
	w.Float32(p.X)
	w.Float32(p.Y)
	w.Float32(p.Z)
	w.Float32(p.OffsetX)
	w.Float32(p.OffsetY)
	w.Float32(p.OffsetZ)
	w.Float32(p.Speed)
	w.Int32(p.Count)
}

func (p *Particle) Unmarshal(r *Reader) error {
	p.Name = r.String(maxParticleNameChars)
	p.X = r.Float32()
	p.Y = r.Float32()
	p.Z = r.Float32()
	p.OffsetX = r.Float32()
	p.OffsetY = r.Float32()
	p.OffsetZ = r.Float32()
	p.Speed = r.Float32()
	p.Count = r.Int32()
	return r.Err()
}

// ChangeGameState changes weather, game mode or another piece of client state selected by
// Reason.
type ChangeGameState struct {
	playClientbound
	Reason uint8
	Value  float32
}

func (*ChangeGameState) ID() int32 { return 0x2b }

func (p *ChangeGameState) Marshal(w *Writer) {
	w.Uint8(p.Reason)
	w.Float32(p.Value)
}

func (p *ChangeGameState) Unmarshal(r *Reader) error {
	p.Reason = r.Uint8()
	p.Value = r.Float32()
	return r.Err()
}

// UpdateSign sets the text of a sign.
type UpdateSign struct {
	playClientbound
	X     int32
	Y     int16
	Z     int32
	Lines [4]string
}

func (*UpdateSign) ID() int32 { return 0x33 }

func (p *UpdateSign) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int16(p.Y)
	w.Int32(p.Z)
	for _, line := range p.Lines {
		w.String(line)
	}
}

func (p *UpdateSign) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int16()
	p.Z = r.Int32()
	for i := range p.Lines {
		p.Lines[i] = r.String(maxSignLineChars)
	}
	return r.Err()
}

// Maps updates part of a held map. MapID is the map item's damage value.
type Maps struct {
	playClientbound
	MapID int32
	Data  []byte
}

func (*Maps) ID() int32 { return 0x34 }

func (p *Maps) Marshal(w *Writer) {
	w.VarInt(p.MapID)
	w.Uint16(uint16(w.length(len(p.Data), math.MaxUint16, "map bytes")))
	w.Raw(p.Data)
}

func (p *Maps) Unmarshal(r *Reader) error {
	p.MapID = r.VarInt()
	p.Data = r.Bytes(int(r.Uint16()))
	return r.Err()
}

// UpdateBlockEntity replaces a block entity's data. Action selects which kind of block entity
// the client applies NBT to.
type UpdateBlockEntity struct {
	playClientbound
	X      int32
	Y      int16
	Z      int32
	Action uint8
	NBT    *NBT
}

func (*UpdateBlockEntity) ID() int32 { return 0x35 }

func (p *UpdateBlockEntity) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int16(p.Y)
	w.Int32(p.Z)
	w.Uint8(p.Action)
	w.NBT(p.NBT)
}

func (p *UpdateBlockEntity) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int16()
	p.Z = r.Int32()
	p.Action = r.Uint8()
	p.NBT = r.NBT()
	return r.Err()
}

// SignEditorOpen opens the sign editor for the sign at a block.
type SignEditorOpen struct {
	playClientbound
	X int32
	Y int32
	Z int32
}

func (*SignEditorOpen) ID() int32 { return 0x36 }

func (p *SignEditorOpen) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int32(p.Y)
	w.Int32(p.Z)
}

func (p *SignEditorOpen) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int32()
	p.Z = r.Int32()
	return r.Err()
}
