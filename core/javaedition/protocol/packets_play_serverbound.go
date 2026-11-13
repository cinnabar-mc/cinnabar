package protocol

import "math"

// Serverbound play packets.

// Field length limits, in UTF-16 units, that 1.7.10 enforces while reading.
const (
	maxChatMessageChars = 100
	maxLocaleChars      = 7
)

// ClientKeepAlive answers a KeepAlive with its key.
type ClientKeepAlive struct {
	playServerbound
	Key int32
}

func (*ClientKeepAlive) ID() int32 { return 0x00 }

func (p *ClientKeepAlive) Marshal(w *Writer) {
	w.Int32(p.Key)
}

func (p *ClientKeepAlive) Unmarshal(r *Reader) error {
	p.Key = r.Int32()
	return r.Err()
}

// ClientChatMessage sends a chat line or a command starting with a slash.
type ClientChatMessage struct {
	playServerbound
	Message string
}

func (*ClientChatMessage) ID() int32 { return 0x01 }

func (p *ClientChatMessage) Marshal(w *Writer) {
	w.String(p.Message)
}

func (p *ClientChatMessage) Unmarshal(r *Reader) error {
	p.Message = r.String(maxChatMessageChars)
	return r.Err()
}

// ClientUseEntity interacts with (Action 0) or attacks (Action 1) an entity.
type ClientUseEntity struct {
	playServerbound
	EntityID int32
	Action   int8
}

func (*ClientUseEntity) ID() int32 { return 0x02 }

func (p *ClientUseEntity) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.Action)
}

func (p *ClientUseEntity) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Action = r.Int8()
	return r.Err()
}

// ClientPlayer reports only whether the player stands on the ground.
type ClientPlayer struct {
	playServerbound
	OnGround bool
}

func (*ClientPlayer) ID() int32 { return 0x03 }

func (p *ClientPlayer) Marshal(w *Writer) {
	w.Bool(p.OnGround)
}

func (p *ClientPlayer) Unmarshal(r *Reader) error {
	p.OnGround = r.Bool()
	return r.Err()
}

// ClientPlayerPosition reports the player's position. Y is the feet position and Stance the eye
// position.
type ClientPlayerPosition struct {
	playServerbound
	X        float64
	Y        float64
	Stance   float64
	Z        float64
	OnGround bool
}

func (*ClientPlayerPosition) ID() int32 { return 0x04 }

func (p *ClientPlayerPosition) Marshal(w *Writer) {
	w.Float64(p.X)
	w.Float64(p.Y)
	w.Float64(p.Stance)
	w.Float64(p.Z)
	w.Bool(p.OnGround)
}

func (p *ClientPlayerPosition) Unmarshal(r *Reader) error {
	p.X = r.Float64()
	p.Y = r.Float64()
	p.Stance = r.Float64()
	p.Z = r.Float64()
	p.OnGround = r.Bool()
	return r.Err()
}

// ClientPlayerLook reports the player's rotation in degrees.
type ClientPlayerLook struct {
	playServerbound
	Yaw      float32
	Pitch    float32
	OnGround bool
}

func (*ClientPlayerLook) ID() int32 { return 0x05 }

func (p *ClientPlayerLook) Marshal(w *Writer) {
	w.Float32(p.Yaw)
	w.Float32(p.Pitch)
	w.Bool(p.OnGround)
}

func (p *ClientPlayerLook) Unmarshal(r *Reader) error {
	p.Yaw = r.Float32()
	p.Pitch = r.Float32()
	p.OnGround = r.Bool()
	return r.Err()
}

// ClientPlayerPositionLook reports the player's position and rotation. Y is the feet position
// and Stance the eye position.
type ClientPlayerPositionLook struct {
	playServerbound
	X        float64
	Y        float64
	Stance   float64
	Z        float64
	Yaw      float32
	Pitch    float32
	OnGround bool
}

func (*ClientPlayerPositionLook) ID() int32 { return 0x06 }

func (p *ClientPlayerPositionLook) Marshal(w *Writer) {
	w.Float64(p.X)
	w.Float64(p.Y)
	w.Float64(p.Stance)
	w.Float64(p.Z)
	w.Float32(p.Yaw)
	w.Float32(p.Pitch)
	w.Bool(p.OnGround)
}

func (p *ClientPlayerPositionLook) Unmarshal(r *Reader) error {
	p.X = r.Float64()
	p.Y = r.Float64()
	p.Stance = r.Float64()
	p.Z = r.Float64()
	p.Yaw = r.Float32()
	p.Pitch = r.Float32()
	p.OnGround = r.Bool()
	return r.Err()
}

// ClientPlayerDigging starts, cancels or finishes breaking a block, or drops or releases the
// held item, as selected by Status.
type ClientPlayerDigging struct {
	playServerbound
	Status uint8
	X      int32
	Y      uint8
	Z      int32
	Face   uint8
}

func (*ClientPlayerDigging) ID() int32 { return 0x07 }

func (p *ClientPlayerDigging) Marshal(w *Writer) {
	w.Uint8(p.Status)
	w.Int32(p.X)
	w.Uint8(p.Y)
	w.Int32(p.Z)
	w.Uint8(p.Face)
}

func (p *ClientPlayerDigging) Unmarshal(r *Reader) error {
	p.Status = r.Uint8()
	p.X = r.Int32()
	p.Y = r.Uint8()
	p.Z = r.Int32()
	p.Face = r.Uint8()
	return r.Err()
}

// ClientBlockPlacement uses the held item on a block face. The cursor fields are the hit point
// within the block in sixteenths.
type ClientBlockPlacement struct {
	playServerbound
	X        int32
	Y        uint8
	Z        int32
	Face     uint8
	HeldItem ItemStack
	CursorX  uint8
	CursorY  uint8
	CursorZ  uint8
}

func (*ClientBlockPlacement) ID() int32 { return 0x08 }

func (p *ClientBlockPlacement) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Uint8(p.Y)
	w.Int32(p.Z)
	w.Uint8(p.Face)
	w.ItemStack(p.HeldItem)
	w.Uint8(p.CursorX)
	w.Uint8(p.CursorY)
	w.Uint8(p.CursorZ)
}

func (p *ClientBlockPlacement) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Uint8()
	p.Z = r.Int32()
	p.Face = r.Uint8()
	p.HeldItem = r.ItemStack()
	p.CursorX = r.Uint8()
	p.CursorY = r.Uint8()
	p.CursorZ = r.Uint8()
	return r.Err()
}

// ClientHeldItemChange selects a hotbar slot.
type ClientHeldItemChange struct {
	playServerbound
	Slot int16
}

func (*ClientHeldItemChange) ID() int32 { return 0x09 }

func (p *ClientHeldItemChange) Marshal(w *Writer) {
	w.Int16(p.Slot)
}

func (p *ClientHeldItemChange) Unmarshal(r *Reader) error {
	p.Slot = r.Int16()
	return r.Err()
}

// ClientAnimation plays one of the player's animations; 1 swings the arm.
type ClientAnimation struct {
	playServerbound
	EntityID  int32
	Animation int8
}

func (*ClientAnimation) ID() int32 { return 0x0a }

func (p *ClientAnimation) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.Animation)
}

func (p *ClientAnimation) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Animation = r.Int8()
	return r.Err()
}

// ClientEntityAction changes sneaking or sprinting, leaves a bed, or drives a ridden horse.
// JumpBoost is the horse jump strength from 0 to 100.
type ClientEntityAction struct {
	playServerbound
	EntityID  int32
	Action    int8
	JumpBoost int32
}

func (*ClientEntityAction) ID() int32 { return 0x0b }

func (p *ClientEntityAction) Marshal(w *Writer) {
	w.Int32(p.EntityID)
	w.Int8(p.Action)
	w.Int32(p.JumpBoost)
}

func (p *ClientEntityAction) Unmarshal(r *Reader) error {
	p.EntityID = r.Int32()
	p.Action = r.Int8()
	p.JumpBoost = r.Int32()
	return r.Err()
}

// ClientSteerVehicle sends movement input while riding.
type ClientSteerVehicle struct {
	playServerbound
	Strafe  float32
	Forward float32
	Jump    bool
	Sneak   bool
}

func (*ClientSteerVehicle) ID() int32 { return 0x0c }

func (p *ClientSteerVehicle) Marshal(w *Writer) {
	w.Float32(p.Strafe)
	w.Float32(p.Forward)
	w.Bool(p.Jump)
	w.Bool(p.Sneak)
}

func (p *ClientSteerVehicle) Unmarshal(r *Reader) error {
	p.Strafe = r.Float32()
	p.Forward = r.Float32()
	p.Jump = r.Bool()
	p.Sneak = r.Bool()
	return r.Err()
}

// ClientCloseWindow closes a container screen.
type ClientCloseWindow struct {
	playServerbound
	WindowID int8
}

func (*ClientCloseWindow) ID() int32 { return 0x0d }

func (p *ClientCloseWindow) Marshal(w *Writer) {
	w.Int8(p.WindowID)
}

func (p *ClientCloseWindow) Unmarshal(r *Reader) error {
	p.WindowID = r.Int8()
	return r.Err()
}

// ClientClickWindow clicks a window slot. Action numbers the click for the server's
// ConfirmTransaction and Item is what the client expects in the slot.
type ClientClickWindow struct {
	playServerbound
	WindowID int8
	Slot     int16
	Button   int8
	Action   int16
	Mode     int8
	Item     ItemStack
}

func (*ClientClickWindow) ID() int32 { return 0x0e }

func (p *ClientClickWindow) Marshal(w *Writer) {
	w.Int8(p.WindowID)
	w.Int16(p.Slot)
	w.Int8(p.Button)
	w.Int16(p.Action)
	w.Int8(p.Mode)
	w.ItemStack(p.Item)
}

func (p *ClientClickWindow) Unmarshal(r *Reader) error {
	p.WindowID = r.Int8()
	p.Slot = r.Int16()
	p.Button = r.Int8()
	p.Action = r.Int16()
	p.Mode = r.Int8()
	p.Item = r.ItemStack()
	return r.Err()
}

// ClientConfirmTransaction acknowledges a rejected ConfirmTransaction.
type ClientConfirmTransaction struct {
	playServerbound
	WindowID int8
	Action   int16
	Accepted bool
}

func (*ClientConfirmTransaction) ID() int32 { return 0x0f }

func (p *ClientConfirmTransaction) Marshal(w *Writer) {
	w.Int8(p.WindowID)
	w.Int16(p.Action)
	w.Bool(p.Accepted)
}

func (p *ClientConfirmTransaction) Unmarshal(r *Reader) error {
	p.WindowID = r.Int8()
	p.Action = r.Int16()
	p.Accepted = r.Bool()
	return r.Err()
}

// ClientCreativeInventoryAction sets a slot in creative mode; a negative slot drops the item.
type ClientCreativeInventoryAction struct {
	playServerbound
	Slot int16
	Item ItemStack
}

func (*ClientCreativeInventoryAction) ID() int32 { return 0x10 }

func (p *ClientCreativeInventoryAction) Marshal(w *Writer) {
	w.Int16(p.Slot)
	w.ItemStack(p.Item)
}

func (p *ClientCreativeInventoryAction) Unmarshal(r *Reader) error {
	p.Slot = r.Int16()
	p.Item = r.ItemStack()
	return r.Err()
}

// ClientEnchantItem picks one of the enchanting table's offers.
type ClientEnchantItem struct {
	playServerbound
	WindowID    int8
	Enchantment int8
}

func (*ClientEnchantItem) ID() int32 { return 0x11 }

func (p *ClientEnchantItem) Marshal(w *Writer) {
	w.Int8(p.WindowID)
	w.Int8(p.Enchantment)
}

func (p *ClientEnchantItem) Unmarshal(r *Reader) error {
	p.WindowID = r.Int8()
	p.Enchantment = r.Int8()
	return r.Err()
}

// ClientUpdateSign submits the text typed into the sign editor.
type ClientUpdateSign struct {
	playServerbound
	X     int32
	Y     int16
	Z     int32
	Lines [4]string
}

func (*ClientUpdateSign) ID() int32 { return 0x12 }

func (p *ClientUpdateSign) Marshal(w *Writer) {
	w.Int32(p.X)
	w.Int16(p.Y)
	w.Int32(p.Z)
	for _, line := range p.Lines {
		w.String(line)
	}
}

func (p *ClientUpdateSign) Unmarshal(r *Reader) error {
	p.X = r.Int32()
	p.Y = r.Int16()
	p.Z = r.Int32()
	for i := range p.Lines {
		p.Lines[i] = r.String(maxSignLineChars)
	}
	return r.Err()
}

// ClientPlayerAbilities reports the player's ability flags and movement speeds.
type ClientPlayerAbilities struct {
	playServerbound
	Invulnerable bool
	Flying       bool
	AllowFlying  bool
	CreativeMode bool
	FlySpeed     float32
	WalkSpeed    float32
}

func (*ClientPlayerAbilities) ID() int32 { return 0x13 }

func (p *ClientPlayerAbilities) Marshal(w *Writer) {
	w.Uint8(abilityFlags(p.Invulnerable, p.Flying, p.AllowFlying, p.CreativeMode))
	w.Float32(p.FlySpeed)
	w.Float32(p.WalkSpeed)
}

func (p *ClientPlayerAbilities) Unmarshal(r *Reader) error {
	flags := r.Uint8()
	p.Invulnerable = flags&abilityInvulnerable != 0
	p.Flying = flags&abilityFlying != 0
	p.AllowFlying = flags&abilityAllowFlying != 0
	p.CreativeMode = flags&abilityCreativeMode != 0
	p.FlySpeed = r.Float32()
	p.WalkSpeed = r.Float32()
	return r.Err()
}

// ClientTabComplete asks for completions of the text typed so far.
type ClientTabComplete struct {
	playServerbound
	Text string
}

func (*ClientTabComplete) ID() int32 { return 0x14 }

func (p *ClientTabComplete) Marshal(w *Writer) {
	w.String(p.Text)
}

func (p *ClientTabComplete) Unmarshal(r *Reader) error {
	p.Text = r.String(MaxStringChars)
	return r.Err()
}

// ClientSettings reports the player's options.
type ClientSettings struct {
	playServerbound
	Locale         string
	ViewDistance   int8
	ChatVisibility int8
	ChatColors     bool
	Difficulty     int8
	ShowCape       bool
}

func (*ClientSettings) ID() int32 { return 0x15 }

func (p *ClientSettings) Marshal(w *Writer) {
	w.String(p.Locale)
	w.Int8(p.ViewDistance)
	w.Int8(p.ChatVisibility)
	w.Bool(p.ChatColors)
	w.Int8(p.Difficulty)
	w.Bool(p.ShowCape)
}

func (p *ClientSettings) Unmarshal(r *Reader) error {
	p.Locale = r.String(maxLocaleChars)
	p.ViewDistance = r.Int8()
	p.ChatVisibility = r.Int8()
	p.ChatColors = r.Bool()
	p.Difficulty = r.Int8()
	p.ShowCape = r.Bool()
	return r.Err()
}

// ClientStatus requests a respawn (Action 0) or the statistics (1), or reports the inventory
// opening (2).
type ClientStatus struct {
	playServerbound
	Action int8
}

func (*ClientStatus) ID() int32 { return 0x16 }

func (p *ClientStatus) Marshal(w *Writer) {
	w.Int8(p.Action)
}

func (p *ClientStatus) Unmarshal(r *Reader) error {
	p.Action = r.Int8()
	return r.Err()
}

// ClientPluginMessage carries data on a named plugin channel. 1.7.10 refuses to send 32767
// bytes or more and reads no data for such a length.
type ClientPluginMessage struct {
	playServerbound
	Channel string
	Data    []byte
}

func (*ClientPluginMessage) ID() int32 { return 0x17 }

func (p *ClientPluginMessage) Marshal(w *Writer) {
	w.String(p.Channel)
	w.Int16(int16(w.length(len(p.Data), math.MaxInt16-1, "plugin message bytes")))
	w.Raw(p.Data)
}

func (p *ClientPluginMessage) Unmarshal(r *Reader) error {
	p.Channel = r.String(maxChannelChars)
	p.Data = nil
	if size := r.Int16(); size > 0 && size < math.MaxInt16 {
		p.Data = r.Bytes(int(size))
	}
	return r.Err()
}
