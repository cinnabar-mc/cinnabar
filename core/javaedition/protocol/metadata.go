package protocol

import "fmt"

// MetadataType is the value type of an entity metadata entry, held in the top three bits of
// the entry's header byte.
type MetadataType uint8

const (
	// MetadataByte entries hold an int8.
	MetadataByte MetadataType = iota
	// MetadataShort entries hold an int16.
	MetadataShort
	// MetadataInt entries hold an int32.
	MetadataInt
	// MetadataFloat entries hold a float32.
	MetadataFloat
	// MetadataString entries hold a string.
	MetadataString
	// MetadataItem entries hold an ItemStack.
	MetadataItem
	// MetadataPosition entries hold a Position.
	MetadataPosition
	// MetadataNone is the one remaining type number. 1.7.10 reads and writes it as a header
	// byte without a payload; its Value is nil.
	MetadataNone
)

// metadataEnd terminates a metadata list. It equals the header of a float entry at index 31,
// which therefore cannot be sent.
const metadataEnd = 127

// Position is a block position as carried by MetadataPosition entries.
type Position struct {
	X, Y, Z int32
}

// MetadataEntry is one entity metadata value. Index is at most 31. Value's Go type follows
// Type: int8, int16, int32, float32, string, ItemStack or Position.
type MetadataEntry struct {
	Index uint8
	Type  MetadataType
	Value any
}

// Metadata is an entity metadata list in wire order. An index may repeat; the client applies
// entries in order, so the last one wins.
type Metadata []MetadataEntry

// Get returns the last entry with the given index.
func (m Metadata) Get(index uint8) (MetadataEntry, bool) {
	for i := len(m) - 1; i >= 0; i-- {
		if m[i].Index == index {
			return m[i], true
		}
	}
	return MetadataEntry{}, false
}

// MetadataValue returns the value at index when the last entry there holds a T.
func MetadataValue[T any](m Metadata, index uint8) (T, bool) {
	entry, ok := m.Get(index)
	if !ok {
		var zero T
		return zero, false
	}
	value, ok := entry.Value.(T)
	return value, ok
}

// Metadata reads entries up to the terminator byte.
func (r *Reader) Metadata() Metadata {
	var list Metadata
	for {
		header := r.Uint8()
		if r.err != nil || header == metadataEnd {
			return list
		}
		entry := MetadataEntry{Index: header & 0x1f, Type: MetadataType(header >> 5)}
		switch entry.Type {
		case MetadataByte:
			entry.Value = r.Int8()
		case MetadataShort:
			entry.Value = r.Int16()
		case MetadataInt:
			entry.Value = r.Int32()
		case MetadataFloat:
			entry.Value = r.Float32()
		case MetadataString:
			entry.Value = r.String(MaxStringChars)
		case MetadataItem:
			entry.Value = r.ItemStack()
		case MetadataPosition:
			entry.Value = Position{X: r.Int32(), Y: r.Int32(), Z: r.Int32()}
		}
		if r.err != nil {
			return nil
		}
		list = append(list, entry)
	}
}

// Metadata writes the entries followed by the terminator. An entry whose Value does not match
// its Type, or whose header would equal the terminator, fails the write.
func (w *Writer) Metadata(m Metadata) {
	for _, entry := range m {
		header := uint8(entry.Type)<<5 | entry.Index&0x1f
		if entry.Type > MetadataNone || entry.Index > 0x1f || header == metadataEnd {
			w.fail(fmt.Errorf("write metadata: type %d at index %d cannot be encoded", entry.Type, entry.Index))
			continue
		}
		w.Uint8(header)
		if !w.metadataValue(entry) {
			w.fail(fmt.Errorf("write metadata: index %d has type %d but holds %T", entry.Index, entry.Type, entry.Value))
		}
	}
	w.Uint8(metadataEnd)
}

// metadataValue writes an entry's payload and reports whether Value had the Go type its Type
// requires.
func (w *Writer) metadataValue(entry MetadataEntry) bool {
	switch entry.Type {
	case MetadataByte:
		v, ok := entry.Value.(int8)
		w.Int8(v)
		return ok
	case MetadataShort:
		v, ok := entry.Value.(int16)
		w.Int16(v)
		return ok
	case MetadataInt:
		v, ok := entry.Value.(int32)
		w.Int32(v)
		return ok
	case MetadataFloat:
		v, ok := entry.Value.(float32)
		w.Float32(v)
		return ok
	case MetadataString:
		v, ok := entry.Value.(string)
		w.String(v)
		return ok
	case MetadataItem:
		v, ok := entry.Value.(ItemStack)
		w.ItemStack(v)
		return ok
	case MetadataPosition:
		v, ok := entry.Value.(Position)
		w.Int32(v.X)
		w.Int32(v.Y)
		w.Int32(v.Z)
		return ok
	}
	return entry.Value == nil
}
