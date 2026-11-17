package protocol

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"math"
	"strings"
	"unicode/utf8"
)

// MaxStringChars is the longest string, in UTF-16 units, that fields without a tighter bound
// accept and the most bytes a written string may take.
const MaxStringChars = math.MaxInt16

// ErrVarIntTooBig reports a VarInt that runs past five bytes.
var ErrVarIntTooBig = errors.New("protocol: VarInt too big")

// Reader decodes big-endian wire values from a byte slice. The first failure is kept: later
// reads return zero values, so callers may decode a whole packet and check Err once.
type Reader struct {
	buf []byte
	off int
	err error
}

// NewReader returns a Reader over b. The Reader does not modify b.
func NewReader(b []byte) *Reader { return &Reader{buf: b} }

// Err returns the first error any read hit, or nil.
func (r *Reader) Err() error { return r.err }

// Remaining returns the number of unread bytes, zero once a read has failed.
func (r *Reader) Remaining() int {
	if r.err != nil {
		return 0
	}
	return len(r.buf) - r.off
}

// fail records err as the Reader's error unless one is already set.
func (r *Reader) fail(err error) {
	if r.err == nil {
		r.err = err
	}
}

// take returns the next n bytes without copying, or nil after recording that what could not
// be read in full.
func (r *Reader) take(n int, what string) []byte {
	if r.err != nil {
		return nil
	}
	if n < 0 || n > len(r.buf)-r.off {
		r.fail(fmt.Errorf("read %s: need %d bytes, have %d: %w", what, n, len(r.buf)-r.off, io.ErrUnexpectedEOF))
		return nil
	}
	b := r.buf[r.off : r.off+n]
	r.off += n
	return b
}

// Uint8 reads an unsigned byte.
func (r *Reader) Uint8() uint8 {
	b := r.take(1, "byte")
	if b == nil {
		return 0
	}
	return b[0]
}

// Int8 reads a signed byte.
func (r *Reader) Int8() int8 { return int8(r.Uint8()) }

// Bool reads a byte and reports whether it is non-zero, as 1.7.10 does.
func (r *Reader) Bool() bool { return r.Uint8() != 0 }

// Uint16 reads an unsigned 16-bit integer.
func (r *Reader) Uint16() uint16 {
	b := r.take(2, "short")
	if b == nil {
		return 0
	}
	return binary.BigEndian.Uint16(b)
}

// Int16 reads a signed 16-bit integer.
func (r *Reader) Int16() int16 { return int16(r.Uint16()) }

// Int32 reads a signed 32-bit integer.
func (r *Reader) Int32() int32 {
	b := r.take(4, "int")
	if b == nil {
		return 0
	}
	return int32(binary.BigEndian.Uint32(b))
}

// Int64 reads a signed 64-bit integer.
func (r *Reader) Int64() int64 {
	b := r.take(8, "long")
	if b == nil {
		return 0
	}
	return int64(binary.BigEndian.Uint64(b))
}

// Float32 reads an IEEE 754 single.
func (r *Reader) Float32() float32 { return math.Float32frombits(uint32(r.Int32())) }

// Float64 reads an IEEE 754 double.
func (r *Reader) Float64() float64 { return math.Float64frombits(uint64(r.Int64())) }

// VarInt reads a variable-length 32-bit integer of at most five bytes. Bits of the fifth byte
// that do not fit are dropped, as 1.7.10 drops them.
func (r *Reader) VarInt() int32 {
	var value uint32
	for i := 0; i < 5; i++ {
		b := r.take(1, "VarInt")
		if b == nil {
			return 0
		}
		value |= uint32(b[0]&0x7f) << (7 * i)
		if b[0]&0x80 == 0 {
			return int32(value)
		}
	}
	// 1.7.10 consumes a sixth byte before it rejects the value.
	if r.take(1, "VarInt") != nil {
		r.fail(ErrVarIntTooBig)
	}
	return 0
}

// Bytes reads n bytes into a new slice. A zero n returns nil.
func (r *Reader) Bytes(n int) []byte {
	b := r.take(n, "bytes")
	if len(b) == 0 {
		return nil
	}
	return append([]byte(nil), b...)
}

// ByteArray16 reads a byte array prefixed by a signed 16-bit length. A negative length is an
// error.
func (r *Reader) ByteArray16() []byte {
	n := r.Int16()
	if n < 0 {
		r.fail(fmt.Errorf("read byte array: negative length %d", n))
		return nil
	}
	return r.Bytes(int(n))
}

// String reads a VarInt byte length followed by UTF-8 text. It fails when the encoding takes
// more than four bytes per allowed character or the text is longer than maxChars UTF-16 units,
// the two limits 1.7.10 enforces. Invalid UTF-8 is replaced with U+FFFD.
func (r *Reader) String(maxChars int) string {
	n := r.VarInt()
	if r.err != nil {
		return ""
	}
	if int(n) > maxChars*4 {
		r.fail(fmt.Errorf("read string: encoded length %d exceeds %d", n, maxChars*4))
		return ""
	}
	if n < 0 {
		r.fail(fmt.Errorf("read string: negative encoded length %d", n))
		return ""
	}
	b := r.take(int(n), "string")
	if r.err != nil {
		return ""
	}
	if units := utf16Length(b); units > maxChars {
		r.fail(fmt.Errorf("read string: length %d exceeds %d", units, maxChars))
		return ""
	}
	return strings.ToValidUTF8(string(b), "�")
}

// utf16Length returns how many UTF-16 units the UTF-8 text b decodes to. For invalid input
// it never counts more than one unit per malformed sequence, so the count is a lower bound.
func utf16Length(b []byte) int {
	units := 0
	for len(b) > 0 {
		c, size := utf8.DecodeRune(b)
		switch {
		case c == utf8.RuneError && size == 1:
			if b[0]&0xc0 != 0x80 {
				units++
			}
		case c > 0xffff:
			units += 2
		default:
			units++
		}
		b = b[size:]
	}
	return units
}

// count validates an element count read from the wire. It fails when n is negative, where
// 1.7.10 fails to allocate the array, and returns n otherwise.
func (r *Reader) count(n int, what string) int {
	if n < 0 {
		r.fail(fmt.Errorf("read %s: negative count %d", what, n))
		return 0
	}
	return n
}

// capacity bounds a slice preallocation for n elements of at least minSize bytes each by the
// bytes actually left, so a hostile count cannot reserve memory the frame does not back.
func (r *Reader) capacity(n, minSize int) int {
	return max(0, min(n, r.Remaining()/minSize))
}
