package protocol

import (
	"encoding/binary"
	"fmt"
	"math"
)

// Writer encodes big-endian wire values into a growing buffer. The first failure is kept and
// reported by Err; later writes still append so callers may encode a whole packet and check
// once.
type Writer struct {
	buf []byte
	err error
}

// NewWriter returns an empty Writer.
func NewWriter() *Writer { return &Writer{} }

// Bytes returns the encoded bytes. The slice is valid until the next write.
func (w *Writer) Bytes() []byte { return w.buf }

// Len returns the number of bytes written so far.
func (w *Writer) Len() int { return len(w.buf) }

// Err returns the first error any write hit, or nil.
func (w *Writer) Err() error { return w.err }

// fail records err as the Writer's error unless one is already set.
func (w *Writer) fail(err error) {
	if w.err == nil {
		w.err = err
	}
}

// Uint8 writes an unsigned byte.
func (w *Writer) Uint8(v uint8) { w.buf = append(w.buf, v) }

// Int8 writes a signed byte.
func (w *Writer) Int8(v int8) { w.Uint8(uint8(v)) }

// Bool writes one for true and zero for false.
func (w *Writer) Bool(v bool) {
	if v {
		w.Uint8(1)
		return
	}
	w.Uint8(0)
}

// Uint16 writes an unsigned 16-bit integer.
func (w *Writer) Uint16(v uint16) { w.buf = binary.BigEndian.AppendUint16(w.buf, v) }

// Int16 writes a signed 16-bit integer.
func (w *Writer) Int16(v int16) { w.Uint16(uint16(v)) }

// Int32 writes a signed 32-bit integer.
func (w *Writer) Int32(v int32) { w.buf = binary.BigEndian.AppendUint32(w.buf, uint32(v)) }

// Int64 writes a signed 64-bit integer.
func (w *Writer) Int64(v int64) { w.buf = binary.BigEndian.AppendUint64(w.buf, uint64(v)) }

// Float32 writes an IEEE 754 single.
func (w *Writer) Float32(v float32) { w.Int32(int32(math.Float32bits(v))) }

// Float64 writes an IEEE 754 double.
func (w *Writer) Float64(v float64) { w.Int64(int64(math.Float64bits(v))) }

// VarInt writes a variable-length 32-bit integer. Negative values take five bytes.
func (w *Writer) VarInt(v int32) { w.buf = AppendVarInt(w.buf, v) }

// AppendVarInt appends the VarInt encoding of v to b.
func AppendVarInt(b []byte, v int32) []byte {
	u := uint32(v)
	for u&^0x7f != 0 {
		b = append(b, byte(u&0x7f)|0x80)
		u >>= 7
	}
	return append(b, byte(u))
}

// VarIntSize returns how many bytes the VarInt encoding of v takes.
func VarIntSize(v int32) int {
	u := uint32(v)
	switch {
	case u&0xffffff80 == 0:
		return 1
	case u&0xffffc000 == 0:
		return 2
	case u&0xffe00000 == 0:
		return 3
	case u&0xf0000000 == 0:
		return 4
	}
	return 5
}

// Raw writes b without a length prefix.
func (w *Writer) Raw(b []byte) { w.buf = append(w.buf, b...) }

// String writes a VarInt byte length followed by the UTF-8 text. Text longer than
// MaxStringChars bytes fails, as it does in 1.7.10.
func (w *Writer) String(s string) {
	if len(s) > MaxStringChars {
		w.fail(fmt.Errorf("write string: %d bytes exceed %d", len(s), MaxStringChars))
		return
	}
	w.VarInt(int32(len(s)))
	w.buf = append(w.buf, s...)
}

// ByteArray16 writes b prefixed by its length as a signed 16-bit integer.
func (w *Writer) ByteArray16(b []byte) {
	w.Int16(int16(w.length(len(b), math.MaxInt16, "byte array")))
	w.Raw(b)
}

// length checks that a collection of n elements fits a count field holding at most limit. It
// returns n, or zero after recording the overflow.
func (w *Writer) length(n, limit int, what string) int {
	if n > limit {
		w.fail(fmt.Errorf("write %s: %d elements exceed %d", what, n, limit))
		return 0
	}
	return n
}
