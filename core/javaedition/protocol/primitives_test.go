package protocol

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"errors"
	"io"
	"math"
	"reflect"
	"strings"
	"testing"
)

func TestVarIntVectors(t *testing.T) {
	vectors := []struct {
		value int32
		wire  []byte
	}{
		{0, []byte{0x00}},
		{1, []byte{0x01}},
		{127, []byte{0x7f}},
		{128, []byte{0x80, 0x01}},
		{255, []byte{0xff, 0x01}},
		{2097151, []byte{0xff, 0xff, 0x7f}},
		{2097152, []byte{0x80, 0x80, 0x80, 0x01}},
		{math.MaxInt32, []byte{0xff, 0xff, 0xff, 0xff, 0x07}},
		{-1, []byte{0xff, 0xff, 0xff, 0xff, 0x0f}},
		{math.MinInt32, []byte{0x80, 0x80, 0x80, 0x80, 0x08}},
	}
	for _, vector := range vectors {
		w := NewWriter()
		w.VarInt(vector.value)
		if !bytes.Equal(w.Bytes(), vector.wire) {
			t.Errorf("VarInt(%d) encoded as % x, want % x", vector.value, w.Bytes(), vector.wire)
		}
		if size := VarIntSize(vector.value); size != len(vector.wire) {
			t.Errorf("VarIntSize(%d) = %d, want %d", vector.value, size, len(vector.wire))
		}
		r := NewReader(vector.wire)
		if got := r.VarInt(); got != vector.value || r.Err() != nil || r.Remaining() != 0 {
			t.Errorf("VarInt(% x) = %d, err %v, %d left; want %d", vector.wire, got, r.Err(), r.Remaining(), vector.value)
		}
	}
}

func TestVarIntRejectsSixBytesAndTruncation(t *testing.T) {
	r := NewReader([]byte{0x80, 0x80, 0x80, 0x80, 0x80, 0x01})
	r.VarInt()
	if !errors.Is(r.Err(), ErrVarIntTooBig) {
		t.Fatalf("six-byte VarInt: err = %v, want ErrVarIntTooBig", r.Err())
	}
	for _, wire := range [][]byte{{}, {0x80}, {0xff, 0xff, 0xff, 0xff, 0x80}} {
		r := NewReader(wire)
		r.VarInt()
		if !errors.Is(r.Err(), io.ErrUnexpectedEOF) {
			t.Errorf("VarInt(% x): err = %v, want unexpected EOF", wire, r.Err())
		}
	}
}

func TestVarIntDropsBitsBeyondThirtyTwo(t *testing.T) {
	r := NewReader([]byte{0xff, 0xff, 0xff, 0xff, 0x7f})
	if got := r.VarInt(); got != -1 || r.Err() != nil {
		t.Fatalf("VarInt with overflowing fifth byte = %d, err %v; want -1", got, r.Err())
	}
}

func TestStringVector(t *testing.T) {
	w := NewWriter()
	w.String("héllo")
	want := []byte{0x06, 'h', 0xc3, 0xa9, 'l', 'l', 'o'}
	if !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("String encoded as % x, want % x", w.Bytes(), want)
	}
	r := NewReader(want)
	if got := r.String(5); got != "héllo" || r.Err() != nil {
		t.Fatalf("String = %q, err %v", got, r.Err())
	}
}

// encodedString returns s with its VarInt byte-length prefix.
func encodedString(s string) []byte {
	return append(AppendVarInt(nil, int32(len(s))), s...)
}

func TestStringLimits(t *testing.T) {
	cases := []struct {
		name     string
		wire     []byte
		maxChars int
		ok       bool
	}{
		{"at character limit", encodedString(strings.Repeat("a", 16)), 16, true},
		{"over character limit", encodedString(strings.Repeat("a", 17)), 16, false},
		{"three-byte characters at limit", encodedString(strings.Repeat("€", 16)), 16, true},
		{"astral characters count two units", encodedString(strings.Repeat("😀", 8)), 16, true},
		{"astral characters over limit", encodedString(strings.Repeat("😀", 9)), 16, false},
		{"byte length over four per character", encodedString(strings.Repeat("a", 65)), 16, false},
		{"negative length", AppendVarInt(nil, -1), 16, false},
		{"truncated body", []byte{0x05, 'a', 'b'}, 16, false},
		{"empty", []byte{0x00}, 16, true},
	}
	for _, tc := range cases {
		r := NewReader(tc.wire)
		r.String(tc.maxChars)
		if (r.Err() == nil) != tc.ok {
			t.Errorf("%s: err = %v, want ok=%v", tc.name, r.Err(), tc.ok)
		}
	}
}

func TestStringReplacesInvalidUTF8(t *testing.T) {
	r := NewReader([]byte{0x03, 'a', 0xff, 'b'})
	if got := r.String(16); got != "a�b" || r.Err() != nil {
		t.Fatalf("String = %q, err %v; want replacement character", got, r.Err())
	}
}

func TestWriteStringTooLong(t *testing.T) {
	w := NewWriter()
	w.String(strings.Repeat("a", MaxStringChars))
	if w.Err() != nil {
		t.Fatalf("string of %d bytes: %v", MaxStringChars, w.Err())
	}
	w.String(strings.Repeat("a", MaxStringChars+1))
	if w.Err() == nil {
		t.Fatalf("string of %d bytes was written", MaxStringChars+1)
	}
}

func TestFixedWidthValuesAreBigEndian(t *testing.T) {
	w := NewWriter()
	w.Bool(true)
	w.Int8(-2)
	w.Uint8(0xfe)
	w.Int16(-2)
	w.Uint16(0xabcd)
	w.Int32(0x01020304)
	w.Int64(0x0102030405060708)
	w.Float32(1)
	w.Float64(-2)
	want := []byte{
		0x01, 0xfe, 0xfe, 0xff, 0xfe, 0xab, 0xcd, 0x01, 0x02, 0x03, 0x04,
		0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
		0x3f, 0x80, 0x00, 0x00,
		0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	}
	if !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("encoded % x, want % x", w.Bytes(), want)
	}
	r := NewReader(want)
	if !r.Bool() || r.Int8() != -2 || r.Uint8() != 0xfe || r.Int16() != -2 || r.Uint16() != 0xabcd ||
		r.Int32() != 0x01020304 || r.Int64() != 0x0102030405060708 || r.Float32() != 1 || r.Float64() != -2 {
		t.Fatal("values did not read back")
	}
	if r.Err() != nil || r.Remaining() != 0 {
		t.Fatalf("err %v, %d bytes left", r.Err(), r.Remaining())
	}
}

func TestBoolReadsAnyNonZeroByteAsTrue(t *testing.T) {
	r := NewReader([]byte{0x02, 0x00})
	if !r.Bool() || r.Bool() {
		t.Fatal("want true for 2 and false for 0")
	}
}

func TestReaderKeepsFirstError(t *testing.T) {
	r := NewReader([]byte{0x01})
	r.Int32()
	first := r.Err()
	if first == nil {
		t.Fatal("short int read succeeded")
	}
	if r.Uint8() != 0 || r.Remaining() != 0 || r.Err() != first {
		t.Fatal("reads after a failure must return zero values and keep the first error")
	}
}

func TestByteArray16(t *testing.T) {
	w := NewWriter()
	w.ByteArray16([]byte{1, 2, 3})
	if want := []byte{0x00, 0x03, 1, 2, 3}; !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("encoded % x, want % x", w.Bytes(), want)
	}
	r := NewReader([]byte{0xff, 0xff})
	r.ByteArray16()
	if r.Err() == nil {
		t.Fatal("negative byte array length was accepted")
	}
}

func TestItemStackVectors(t *testing.T) {
	w := NewWriter()
	w.ItemStack(EmptyItemStack())
	if want := []byte{0xff, 0xff}; !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("empty stack encoded as % x, want % x", w.Bytes(), want)
	}

	w = NewWriter()
	w.ItemStack(ItemStack{ID: 276, Count: 1, Damage: 5})
	want := []byte{0x01, 0x14, 0x01, 0x00, 0x05, 0xff, 0xff}
	if !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("stack encoded as % x, want % x", w.Bytes(), want)
	}
	r := NewReader(want)
	if got := r.ItemStack(); !reflect.DeepEqual(got, ItemStack{ID: 276, Count: 1, Damage: 5}) || r.Err() != nil {
		t.Fatalf("stack read as %+v, err %v", got, r.Err())
	}

	r = NewReader([]byte{0xff, 0xfb})
	if got := r.ItemStack(); !got.Empty() || r.Err() != nil || r.Remaining() != 0 {
		t.Fatalf("negative id read as %+v, err %v; want an empty stack", got, r.Err())
	}
}

// mustNBT encodes root for the wire or fails the test.
func mustNBT(t testing.TB, root map[string]any) *NBT {
	t.Helper()
	n, err := NewNBT(root)
	if err != nil {
		t.Fatalf("NewNBT: %v", err)
	}
	return n
}

func TestItemStackWithNBTRoundTrips(t *testing.T) {
	stack := ItemStack{ID: 276, Count: 1, Damage: 12, NBT: mustNBT(t, map[string]any{
		"display": map[string]any{"Name": "Sting"},
		"ench":    []any{map[string]any{"id": int16(16), "lvl": int16(5)}},
	})}
	w := NewWriter()
	w.ItemStack(stack)
	if w.Err() != nil {
		t.Fatal(w.Err())
	}
	r := NewReader(w.Bytes())
	got := r.ItemStack()
	if r.Err() != nil || r.Remaining() != 0 {
		t.Fatalf("err %v, %d bytes left", r.Err(), r.Remaining())
	}
	if !reflect.DeepEqual(got, stack) {
		t.Fatalf("stack read as %+v, want %+v", got, stack)
	}
	root, err := got.NBT.Decode()
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]any{
		"display": map[string]any{"Name": "Sting"},
		"ench":    []any{map[string]any{"id": int16(16), "lvl": int16(5)}},
	}
	if !reflect.DeepEqual(root, want) {
		t.Fatalf("decoded NBT %#v, want %#v", root, want)
	}
}

// gzipBytes compresses raw at the given level.
func gzipBytes(t testing.TB, raw []byte, level int) []byte {
	t.Helper()
	var out bytes.Buffer
	zw, err := gzip.NewWriterLevel(&out, level)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := zw.Write(raw); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

// nbtName encodes a tag name or string payload: a 16-bit length and the bytes.
func nbtName(name string) []byte {
	return append(binary.BigEndian.AppendUint16(nil, uint16(len(name))), name...)
}

// nbtRoot wraps a compound payload, which must end with the end tag, in a named root tag.
func nbtRoot(payload ...byte) []byte {
	return append(append([]byte{tagCompound}, nbtName("")...), payload...)
}

func TestNBTKeepsWireBytes(t *testing.T) {
	raw := nbtRoot(append(append([]byte{tagByte}, nbtName("b")...), 7, tagEnd)...)
	// A level this package never uses stands in for a peer's own compressor.
	compressed := gzipBytes(t, raw, gzip.NoCompression)
	wire := append(binary.BigEndian.AppendUint16(nil, uint16(len(compressed))), compressed...)

	r := NewReader(wire)
	n := r.NBT()
	if r.Err() != nil || n == nil {
		t.Fatalf("NBT read: %v", r.Err())
	}
	if !bytes.Equal(n.Compressed(), compressed) || !bytes.Equal(n.Raw(), raw) {
		t.Fatal("NBT did not keep the bytes it was read from")
	}
	root, err := n.Decode()
	if err != nil || !reflect.DeepEqual(root, map[string]any{"b": uint8(7)}) {
		t.Fatalf("Decode = %#v, %v", root, err)
	}
	w := NewWriter()
	w.NBT(n)
	if !bytes.Equal(w.Bytes(), wire) {
		t.Fatal("NBT was not written back byte for byte")
	}
}

func TestNBTAbsent(t *testing.T) {
	w := NewWriter()
	w.NBT(nil)
	if want := []byte{0xff, 0xff}; !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("absent NBT encoded as % x", w.Bytes())
	}
	r := NewReader(w.Bytes())
	n := r.NBT()
	if n != nil || r.Err() != nil {
		t.Fatalf("absent NBT read as %v, err %v", n, r.Err())
	}
	if root, err := n.Decode(); root != nil || err != nil || n.Raw() != nil || n.Compressed() != nil {
		t.Fatalf("absent NBT decoded as %v, err %v; want nothing", root, err)
	}
}

// nestedCompounds returns a root compound holding depth compounds inside one another.
func nestedCompounds(depth int) []byte {
	var payload []byte
	for i := 0; i < depth; i++ {
		payload = append(append(payload, tagCompound), nbtName("c")...)
	}
	// One end tag per nested compound, then the root's.
	payload = append(payload, make([]byte, depth+1)...)
	return nbtRoot(payload...)
}

// byteArrayCompound returns a root compound holding one byte array of the given length under
// a one-character name.
func byteArrayCompound(length int) []byte {
	payload := append(append([]byte{tagByteArray}, nbtName("a")...), binary.BigEndian.AppendUint32(nil, uint32(length))...)
	payload = append(payload, make([]byte, length)...)
	return nbtRoot(append(payload, tagEnd)...)
}

func TestNBTValidation(t *testing.T) {
	// The name "a" accounts for two bytes, so this array fills the client's budget exactly.
	const largestArray = maxNBTAccounted - 2
	listOf := func(element byte, count uint32, elements ...byte) []byte {
		payload := append(append([]byte{tagList}, nbtName("l")...), element)
		payload = append(binary.BigEndian.AppendUint32(payload, count), elements...)
		return nbtRoot(append(payload, tagEnd)...)
	}
	cases := []struct {
		name string
		raw  []byte
		ok   bool
	}{
		{"empty compound", nbtRoot(tagEnd), true},
		{"root is end tag", []byte{tagEnd}, false},
		{"root is not a compound", append(append([]byte{tagInt}, nbtName("")...), 0, 0, 0, 1), false},
		{"missing end tag", nbtRoot(), false},
		{"unknown tag id", nbtRoot(append(append([]byte{12}, nbtName("x")...), tagEnd)...), false},
		{"nesting at limit", nestedCompounds(maxNBTDepth), true},
		{"nesting over limit", nestedCompounds(maxNBTDepth + 1), false},
		{"size at budget", byteArrayCompound(largestArray), true},
		{"size over budget", byteArrayCompound(largestArray + 1), false},
		{"list of empty compounds", listOf(tagCompound, 3, tagEnd, tagEnd, tagEnd), true},
		{"list of end tags has no elements to read", listOf(tagEnd, math.MaxInt32), true},
		{"negative list length reads nothing", listOf(tagInt, math.MaxUint32), true},
		{"list shorter than its length", listOf(tagInt, 2, 0, 0, 0, 1), false},
		{"trailing bytes after root", append(nbtRoot(tagEnd), 0xde, 0xad), true},
	}
	for _, tc := range cases {
		_, err := ParseNBT(gzipBytes(t, tc.raw, gzip.BestCompression))
		if (err == nil) != tc.ok {
			t.Errorf("%s: err = %v, want ok=%v", tc.name, err, tc.ok)
		}
	}
}

func TestNBTRejectsBadGzip(t *testing.T) {
	if _, err := ParseNBT([]byte("not gzip at all")); err == nil {
		t.Fatal("garbage was accepted as NBT")
	}
	compressed := gzipBytes(t, nbtRoot(tagEnd), gzip.BestCompression)
	if _, err := ParseNBT(compressed[:len(compressed)/2]); err == nil {
		t.Fatal("NBT cut inside its deflate stream was accepted")
	}
}

// TestNBTIgnoresDamageAfterRoot covers the client never reading past the root tag: a wrong
// checksum or bytes after the gzip member do not fail the packet.
func TestNBTIgnoresDamageAfterRoot(t *testing.T) {
	compressed := gzipBytes(t, nbtRoot(tagEnd), gzip.BestCompression)
	badChecksum := append([]byte(nil), compressed...)
	badChecksum[len(badChecksum)-8] ^= 0xff
	if _, err := ParseNBT(badChecksum); err != nil {
		t.Errorf("wrong gzip checksum: %v", err)
	}
	if _, err := ParseNBT(append(append([]byte(nil), compressed...), 1, 2, 3)); err != nil {
		t.Errorf("bytes after the gzip member: %v", err)
	}
}

func TestNewNBTRejectsOversizedCompound(t *testing.T) {
	// Random-looking bytes do not compress, so the result cannot fit the 16-bit length.
	noise := make([]byte, 2*math.MaxInt16)
	state := uint32(1)
	for i := range noise {
		state = state*1664525 + 1013904223
		noise[i] = byte(state >> 24)
	}
	if _, err := NewNBT(map[string]any{"noise": string(noise[:1024])}); err != nil {
		t.Fatalf("compound that fits: %v", err)
	}
	if _, err := NewNBT(map[string]any{"noise": string(noise[:math.MaxInt16]), "more": string(noise[math.MaxInt16:])}); err == nil {
		t.Fatal("compound larger than the length prefix allows was accepted")
	}
}

func TestMetadataEveryType(t *testing.T) {
	list := Metadata{
		{Index: 0, Type: MetadataByte, Value: int8(-3)},
		{Index: 1, Type: MetadataShort, Value: int16(300)},
		{Index: 2, Type: MetadataInt, Value: int32(-70000)},
		{Index: 3, Type: MetadataFloat, Value: float32(20.5)},
		{Index: 4, Type: MetadataString, Value: "Dinnerbone"},
		{Index: 5, Type: MetadataItem, Value: ItemStack{ID: 1, Count: 64, Damage: 3}},
		{Index: 6, Type: MetadataPosition, Value: Position{X: 1, Y: -2, Z: 3}},
		{Index: 31, Type: MetadataNone},
	}
	w := NewWriter()
	w.Metadata(list)
	if w.Err() != nil {
		t.Fatal(w.Err())
	}
	want := []byte{
		0x00, 0xfd,
		0x21, 0x01, 0x2c,
		0x42, 0xff, 0xfe, 0xee, 0x90,
		0x63, 0x41, 0xa4, 0x00, 0x00,
		0x84, 0x0a, 'D', 'i', 'n', 'n', 'e', 'r', 'b', 'o', 'n', 'e',
		0xa5, 0x00, 0x01, 0x40, 0x00, 0x03, 0xff, 0xff,
		0xc6, 0x00, 0x00, 0x00, 0x01, 0xff, 0xff, 0xff, 0xfe, 0x00, 0x00, 0x00, 0x03,
		0xff,
		0x7f,
	}
	if !bytes.Equal(w.Bytes(), want) {
		t.Fatalf("metadata encoded as\n% x, want\n% x", w.Bytes(), want)
	}
	r := NewReader(want)
	got := r.Metadata()
	if r.Err() != nil || r.Remaining() != 0 {
		t.Fatalf("err %v, %d bytes left", r.Err(), r.Remaining())
	}
	if !reflect.DeepEqual(got, list) {
		t.Fatalf("metadata read as %+v, want %+v", got, list)
	}
}

func TestMetadataLookup(t *testing.T) {
	list := Metadata{
		{Index: 6, Type: MetadataFloat, Value: float32(10)},
		{Index: 6, Type: MetadataFloat, Value: float32(4)},
		{Index: 10, Type: MetadataString, Value: "name"},
	}
	if health, ok := MetadataValue[float32](list, 6); !ok || health != 4 {
		t.Fatalf("MetadataValue = %v, %v; want the last entry at the index", health, ok)
	}
	if _, ok := MetadataValue[int8](list, 10); ok {
		t.Fatal("MetadataValue returned a value of the wrong type")
	}
	if _, ok := list.Get(3); ok {
		t.Fatal("Get found an index that is not in the list")
	}
}

func TestMetadataWriteRejectsUnencodableEntries(t *testing.T) {
	bad := []MetadataEntry{
		{Index: 0, Type: MetadataByte, Value: int32(1)},
		{Index: 31, Type: MetadataFloat, Value: float32(1)},
		{Index: 32, Type: MetadataByte, Value: int8(1)},
		{Index: 0, Type: MetadataNone + 1},
	}
	for _, entry := range bad {
		w := NewWriter()
		w.Metadata(Metadata{entry})
		if w.Err() == nil {
			t.Errorf("entry %+v was written", entry)
		}
	}
}

func TestMetadataMissingTerminator(t *testing.T) {
	r := NewReader([]byte{0x00, 0x01})
	r.Metadata()
	if !errors.Is(r.Err(), io.ErrUnexpectedEOF) {
		t.Fatalf("err = %v, want unexpected EOF", r.Err())
	}
}
