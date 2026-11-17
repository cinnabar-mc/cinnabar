package protocol

// ItemStack is an inventory slot. A negative ID is an empty slot and carries no other fields
// on the wire. The zero value is not empty: it is item id 0 with a count of 0.
type ItemStack struct {
	ID     int16
	Count  int8
	Damage int16
	// NBT is the stack's tag compound, nil when it has none.
	NBT *NBT
}

// EmptyItemStack returns the empty slot as 1.7.10 writes it.
func EmptyItemStack() ItemStack { return ItemStack{ID: -1} }

// Empty reports whether the slot holds nothing.
func (s ItemStack) Empty() bool { return s.ID < 0 }

// ItemStack reads a slot: the item id, then count, damage and optional NBT unless the id is
// negative.
func (r *Reader) ItemStack() ItemStack {
	stack := ItemStack{ID: r.Int16()}
	if stack.ID < 0 {
		return stack
	}
	stack.Count = r.Int8()
	stack.Damage = r.Int16()
	stack.NBT = r.NBT()
	return stack
}

// ItemStack writes a slot. An empty slot writes only its negative id.
func (w *Writer) ItemStack(s ItemStack) {
	w.Int16(s.ID)
	if s.ID < 0 {
		return
	}
	w.Int8(s.Count)
	w.Int16(s.Damage)
	w.NBT(s.NBT)
}
