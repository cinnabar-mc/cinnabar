package protocol

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"math"

	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

const (
	// maxNBTAccounted is the size budget 1.7.10 gives a compound read from the network.
	maxNBTAccounted = 0x200000
	// maxNBTDepth is the deepest compound or list nesting 1.7.10 reads.
	maxNBTDepth = 512
	// maxNBTInflated bounds the uncompressed size of a compound. Deflate cannot expand the
	// largest payload the 16-bit length prefix admits beyond this, so no valid compound is cut.
	maxNBTInflated = 64 << 20
)

// NBT tag ids used by the validation walk.
const (
	tagEnd = iota
	tagByte
	tagShort
	tagInt
	tagLong
	tagFloat
	tagDouble
	tagByteArray
	tagString
	tagList
	tagCompound
	tagIntArray
)

// NBT is one compound tag in its 1.7.10 network form: big-endian NBT with a named root,
// gzip-compressed. It keeps the bytes it was built from, so a compound read from the wire is
// written back unchanged. A nil *NBT means the compound is absent. The value is immutable;
// build one with NewNBT or ParseNBT.
type NBT struct {
	compressed []byte
}

// NewNBT encodes root as a compound for the wire. It fails when root cannot be encoded or its
// compressed form does not fit the 16-bit length prefix.
func NewNBT(root map[string]any) (*NBT, error) {
	if root == nil {
		root = map[string]any{}
	}
	raw, err := nbt.MarshalEncoding(root, nbt.BigEndian)
	if err != nil {
		return nil, fmt.Errorf("encode NBT: %w", err)
	}
	var out bytes.Buffer
	zw := gzip.NewWriter(&out)
	if _, err := zw.Write(raw); err != nil {
		return nil, fmt.Errorf("compress NBT: %w", err)
	}
	if err := zw.Close(); err != nil {
		return nil, fmt.Errorf("compress NBT: %w", err)
	}
	if out.Len() > math.MaxInt16 {
		return nil, fmt.Errorf("compress NBT: %d bytes exceed %d", out.Len(), math.MaxInt16)
	}
	return &NBT{compressed: out.Bytes()}, nil
}

// ParseNBT accepts compressed bytes as they appear on the wire after the length prefix. It
// applies the checks 1.7.10 applies while reading: the stream must inflate as far as the root
// tag reaches, the root must be a compound, and the tags must stay within the client's depth
// and size budget.
func ParseNBT(compressed []byte) (*NBT, error) {
	raw, inflateErr := inflateNBT(compressed)
	if errors.Is(inflateErr, errNBTInflatedTooLarge) {
		return nil, inflateErr
	}
	if err := validateNBT(raw); err != nil {
		if inflateErr != nil {
			return nil, inflateErr
		}
		return nil, err
	}
	return &NBT{compressed: append([]byte(nil), compressed...)}, nil
}

// Compressed returns the gzip bytes written to the wire, nil for an absent compound. Callers
// must not modify them.
func (n *NBT) Compressed() []byte {
	if n == nil {
		return nil
	}
	return n.compressed
}

// Raw returns the uncompressed big-endian NBT, starting at the root tag id, or nil for an
// absent compound.
func (n *NBT) Raw() []byte {
	if n == nil {
		return nil
	}
	raw, _ := inflateNBT(n.compressed)
	return raw
}

// Decode returns the root compound as Go values: TAG_Byte as uint8, TAG_Short as int16,
// TAG_Int as int32, TAG_Long as int64, the float tags as float32 and float64, TAG_String as
// string, byte and int arrays as Go arrays, lists as slices and compounds as map[string]any.
// An absent compound decodes to a nil map. A compound the client accepts but this decoder
// cannot represent is an error here, not when the packet is read.
func (n *NBT) Decode() (map[string]any, error) {
	if n == nil {
		return nil, nil
	}
	var root map[string]any
	if err := nbt.UnmarshalEncoding(n.Raw(), &root, nbt.BigEndian); err != nil {
		return nil, fmt.Errorf("decode NBT: %w", err)
	}
	return root, nil
}

// errNBTInflatedTooLarge reports a compound that inflates past maxNBTInflated.
var errNBTInflatedTooLarge = fmt.Errorf("inflate NBT: output exceeds %d bytes", maxNBTInflated)

// inflateNBT gunzips the first gzip member of a compound. The client stops reading at the end
// of the root tag and never checks what follows, so the bytes that inflated before a failure
// are returned together with the error for the caller to judge.
func inflateNBT(compressed []byte) ([]byte, error) {
	zr, err := gzip.NewReader(bytes.NewReader(compressed))
	if err != nil {
		return nil, fmt.Errorf("inflate NBT: %w", err)
	}
	zr.Multistream(false)
	raw, err := io.ReadAll(io.LimitReader(zr, maxNBTInflated+1))
	if len(raw) > maxNBTInflated {
		return nil, errNBTInflatedTooLarge
	}
	if err != nil {
		return raw, fmt.Errorf("inflate NBT: %w", err)
	}
	return raw, nil
}

// errNBTTruncated reports NBT that ends inside a tag.
var errNBTTruncated = fmt.Errorf("NBT ends early: %w", io.ErrUnexpectedEOF)

// nbtWalk steps over uncompressed NBT without building values, keeping the running size total
// 1.7.10 keeps.
type nbtWalk struct {
	data      []byte
	off       int
	accounted int64
}

// validateNBT checks that raw starts with a compound root the 1.7.10 client would read:
// known tag ids, nesting of at most maxNBTDepth, and an accounted size within maxNBTAccounted.
// Bytes after the root are ignored, as the client ignores them.
func validateNBT(raw []byte) error {
	w := &nbtWalk{data: raw}
	id, err := w.next(1)
	if err != nil {
		return err
	}
	if id[0] != tagCompound {
		return errors.New("NBT root tag must be a named compound tag")
	}
	if _, err := w.text(); err != nil {
		return err
	}
	return w.payload(tagCompound, 0)
}

// next returns the following n bytes of the walk.
func (w *nbtWalk) next(n int) ([]byte, error) {
	if n < 0 || n > len(w.data)-w.off {
		return nil, errNBTTruncated
	}
	b := w.data[w.off : w.off+n]
	w.off += n
	return b, nil
}

// account adds a tag's size in bits to the running total the way 1.7.10 does, whole bytes
// per call, and fails once the total passes the budget.
func (w *nbtWalk) account(bits int64) error {
	w.accounted += bits / 8
	if w.accounted > maxNBTAccounted {
		return fmt.Errorf("NBT tag is too big: %d bytes accounted, %d allowed", w.accounted, maxNBTAccounted)
	}
	return nil
}

// text skips a length-prefixed string and returns its length in UTF-16 units. Strings use
// Java's modified UTF-8, where every unit starts with a non-continuation byte.
func (w *nbtWalk) text() (int64, error) {
	prefix, err := w.next(2)
	if err != nil {
		return 0, err
	}
	b, err := w.next(int(binary.BigEndian.Uint16(prefix)))
	if err != nil {
		return 0, err
	}
	var units int64
	for _, c := range b {
		if c&0xc0 != 0x80 {
			units++
		}
	}
	return units, nil
}

// length reads a signed 32-bit element count.
func (w *nbtWalk) length() (int64, error) {
	b, err := w.next(4)
	if err != nil {
		return 0, err
	}
	return int64(int32(binary.BigEndian.Uint32(b))), nil
}

// fixed accounts for and skips a tag of size bytes.
func (w *nbtWalk) fixed(size int) error {
	if err := w.account(int64(size) * 8); err != nil {
		return err
	}
	_, err := w.next(size)
	return err
}

// payload skips the payload of a tag with the given id nested depth levels below the root.
func (w *nbtWalk) payload(id byte, depth int) error {
	switch id {
	case tagEnd:
		return nil
	case tagByte:
		return w.fixed(1)
	case tagShort:
		return w.fixed(2)
	case tagInt, tagFloat:
		return w.fixed(4)
	case tagLong, tagDouble:
		return w.fixed(8)
	case tagByteArray, tagIntArray:
		return w.array(id)
	case tagString:
		units, err := w.text()
		if err != nil {
			return err
		}
		return w.account(16 * units)
	case tagList:
		return w.list(depth)
	case tagCompound:
		return w.compound(depth)
	}
	return fmt.Errorf("NBT tag id %d is unknown", id)
}

// array skips a byte or int array.
func (w *nbtWalk) array(id byte) error {
	n, err := w.length()
	if err != nil {
		return err
	}
	if n < 0 {
		return fmt.Errorf("NBT array has negative length %d", n)
	}
	width := int64(1)
	if id == tagIntArray {
		width = 4
	}
	if n*width > int64(len(w.data)-w.off) {
		return errNBTTruncated
	}
	if err := w.account(8 * width * n); err != nil {
		return err
	}
	_, err = w.next(int(n * width))
	return err
}

// list skips a list. A negative length reads no elements. Elements of the end tag carry no
// payload, so their count is not walked.
func (w *nbtWalk) list(depth int) error {
	if depth > maxNBTDepth {
		return fmt.Errorf("NBT nests deeper than %d", maxNBTDepth)
	}
	if err := w.account(8); err != nil {
		return err
	}
	element, err := w.next(1)
	if err != nil {
		return err
	}
	n, err := w.length()
	if err != nil {
		return err
	}
	if element[0] == tagEnd {
		return nil
	}
	for i := int64(0); i < n; i++ {
		if err := w.payload(element[0], depth+1); err != nil {
			return err
		}
	}
	return nil
}

// compound skips named tags up to the end tag.
func (w *nbtWalk) compound(depth int) error {
	if depth > maxNBTDepth {
		return fmt.Errorf("NBT nests deeper than %d", maxNBTDepth)
	}
	for {
		id, err := w.next(1)
		if err != nil {
			return err
		}
		if id[0] == tagEnd {
			return nil
		}
		units, err := w.text()
		if err != nil {
			return err
		}
		if err := w.account(16 * units); err != nil {
			return err
		}
		if err := w.payload(id[0], depth+1); err != nil {
			return err
		}
	}
}

// NBT reads an optional compound: a signed 16-bit length, negative when absent, then that
// many gzip bytes.
func (r *Reader) NBT() *NBT {
	n := r.Int16()
	if r.err != nil || n < 0 {
		return nil
	}
	compressed := r.take(int(n), "NBT")
	if r.err != nil {
		return nil
	}
	parsed, err := ParseNBT(compressed)
	if err != nil {
		r.fail(fmt.Errorf("read NBT: %w", err))
		return nil
	}
	return parsed
}

// NBT writes an optional compound; nil writes the absent marker.
func (w *Writer) NBT(n *NBT) {
	if n == nil {
		w.Int16(-1)
		return
	}
	w.Int16(int16(w.length(len(n.compressed), math.MaxInt16, "NBT bytes")))
	w.Raw(n.compressed)
}
