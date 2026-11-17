package protocol

import "math"

// Inventory packets. Window 0 is the player's own inventory.

// maxWindowTitleChars is the longest window title 1.7.10 reads.
const maxWindowTitleChars = 32

// windowTypeHorse is the OpenWindow type that carries the horse's entity id.
const windowTypeHorse = 11

// HeldItemChange selects a hotbar slot.
type HeldItemChange struct {
	playClientbound
	Slot int8
}

func (*HeldItemChange) ID() int32 { return 0x09 }

func (p *HeldItemChange) Marshal(w *Writer) {
	w.Int8(p.Slot)
}

func (p *HeldItemChange) Unmarshal(r *Reader) error {
	p.Slot = r.Int8()
	return r.Err()
}

// OpenWindow opens a container screen. EntityID is on the wire only for the horse inventory,
// type 11.
type OpenWindow struct {
	playClientbound
	WindowID uint8
	Type     uint8
	Title    string
	Slots    uint8
	// UseProvidedTitle tells the client to show Title instead of its own name for the window.
	UseProvidedTitle bool
	EntityID         int32
}

func (*OpenWindow) ID() int32 { return 0x2d }

func (p *OpenWindow) Marshal(w *Writer) {
	w.Uint8(p.WindowID)
	w.Uint8(p.Type)
	w.String(p.Title)
	w.Uint8(p.Slots)
	w.Bool(p.UseProvidedTitle)
	if p.Type == windowTypeHorse {
		w.Int32(p.EntityID)
	}
}

func (p *OpenWindow) Unmarshal(r *Reader) error {
	p.WindowID = r.Uint8()
	p.Type = r.Uint8()
	p.Title = r.String(maxWindowTitleChars)
	p.Slots = r.Uint8()
	p.UseProvidedTitle = r.Bool()
	p.EntityID = 0
	if p.Type == windowTypeHorse {
		p.EntityID = r.Int32()
	}
	return r.Err()
}

// CloseWindow closes a container screen.
type CloseWindow struct {
	playClientbound
	WindowID uint8
}

func (*CloseWindow) ID() int32 { return 0x2e }

func (p *CloseWindow) Marshal(w *Writer) {
	w.Uint8(p.WindowID)
}

func (p *CloseWindow) Unmarshal(r *Reader) error {
	p.WindowID = r.Uint8()
	return r.Err()
}

// SetSlot sets one slot of a window. Window -1 sets the item on the cursor.
type SetSlot struct {
	playClientbound
	WindowID int8
	Slot     int16
	Item     ItemStack
}

func (*SetSlot) ID() int32 { return 0x2f }

func (p *SetSlot) Marshal(w *Writer) {
	w.Int8(p.WindowID)
	w.Int16(p.Slot)
	w.ItemStack(p.Item)
}

func (p *SetSlot) Unmarshal(r *Reader) error {
	p.WindowID = r.Int8()
	p.Slot = r.Int16()
	p.Item = r.ItemStack()
	return r.Err()
}

// WindowItems sets every slot of a window.
type WindowItems struct {
	playClientbound
	WindowID uint8
	Items    []ItemStack
}

func (*WindowItems) ID() int32 { return 0x30 }

func (p *WindowItems) Marshal(w *Writer) {
	w.Uint8(p.WindowID)
	w.Int16(int16(w.length(len(p.Items), math.MaxInt16, "window items")))
	for _, item := range p.Items {
		w.ItemStack(item)
	}
}

func (p *WindowItems) Unmarshal(r *Reader) error {
	p.WindowID = r.Uint8()
	count := r.count(int(r.Int16()), "window items")
	p.Items = nil
	if count > 0 {
		p.Items = make([]ItemStack, 0, r.capacity(count, 2))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Items = append(p.Items, r.ItemStack())
	}
	return r.Err()
}

// WindowProperty updates a progress value of a window, such as a furnace's burn time.
type WindowProperty struct {
	playClientbound
	WindowID uint8
	Property int16
	Value    int16
}

func (*WindowProperty) ID() int32 { return 0x31 }

func (p *WindowProperty) Marshal(w *Writer) {
	w.Uint8(p.WindowID)
	w.Int16(p.Property)
	w.Int16(p.Value)
}

func (p *WindowProperty) Unmarshal(r *Reader) error {
	p.WindowID = r.Uint8()
	p.Property = r.Int16()
	p.Value = r.Int16()
	return r.Err()
}

// ConfirmTransaction answers a window click. The client echoes a rejected one back.
type ConfirmTransaction struct {
	playClientbound
	WindowID uint8
	Action   int16
	Accepted bool
}

func (*ConfirmTransaction) ID() int32 { return 0x32 }

func (p *ConfirmTransaction) Marshal(w *Writer) {
	w.Uint8(p.WindowID)
	w.Int16(p.Action)
	w.Bool(p.Accepted)
}

func (p *ConfirmTransaction) Unmarshal(r *Reader) error {
	p.WindowID = r.Uint8()
	p.Action = r.Int16()
	p.Accepted = r.Bool()
	return r.Err()
}
