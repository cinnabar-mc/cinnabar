package protocol

import (
	"bytes"
	"compress/zlib"
	"errors"
	"reflect"
	"testing"
)

// deflate returns raw as a zlib stream.
func deflate(t testing.TB, raw []byte) []byte {
	t.Helper()
	var out bytes.Buffer
	zw := zlib.NewWriter(&out)
	if _, err := zw.Write(raw); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

// pattern returns n bytes that differ by position, so misplaced slices show up.
func pattern(n int) []byte {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte(i*7 + i>>8)
	}
	return b
}

func TestChunkDataInflate(t *testing.T) {
	cases := []struct {
		name     string
		groundUp bool
		mask     uint16
		size     int
	}{
		{"ground-up continuous", true, 0x8001, 2*12288 + 256},
		{"partial update", false, 0x0010, 12288},
		{"unload", true, 0, 256},
	}
	for _, tc := range cases {
		raw := pattern(tc.size)
		p := &ChunkData{GroundUpContinuous: tc.groundUp, PrimaryBitmask: tc.mask, Compressed: deflate(t, raw)}
		if p.InflatedSize() != tc.size {
			t.Errorf("%s: InflatedSize = %d, want %d", tc.name, p.InflatedSize(), tc.size)
		}
		got, err := p.Inflate()
		if err != nil || !bytes.Equal(got, raw) {
			t.Errorf("%s: Inflate returned %d bytes, err %v", tc.name, len(got), err)
		}
	}
}

func TestChunkDataInflateFollowsTheClientBuffer(t *testing.T) {
	short := &ChunkData{PrimaryBitmask: 1, Compressed: deflate(t, []byte{1, 2, 3})}
	got, err := short.Inflate()
	if err != nil || len(got) != 12288 || !bytes.Equal(got[:4], []byte{1, 2, 3, 0}) {
		t.Fatalf("short stream: %d bytes, err %v; want zero padding", len(got), err)
	}

	long := &ChunkData{PrimaryBitmask: 1, Compressed: deflate(t, pattern(20000))}
	got, err = long.Inflate()
	if err != nil || !bytes.Equal(got, pattern(20000)[:12288]) {
		t.Fatalf("long stream: %d bytes, err %v; want it cut to the buffer", len(got), err)
	}

	compressed := deflate(t, pattern(12288))
	cut := &ChunkData{PrimaryBitmask: 1, Compressed: compressed[:len(compressed)/2]}
	if _, err := cut.Inflate(); err != nil {
		t.Fatalf("stream that ends early: %v", err)
	}

	empty := &ChunkData{PrimaryBitmask: 1}
	if got, err := empty.Inflate(); err != nil || len(got) != 12288 {
		t.Fatalf("empty stream: %d bytes, err %v", len(got), err)
	}
}

func TestChunkDataInflateRejectsCorruptData(t *testing.T) {
	garbage := &ChunkData{PrimaryBitmask: 1, Compressed: []byte{0xde, 0xad, 0xbe, 0xef}}
	if _, err := garbage.Inflate(); !errors.Is(err, ErrBadChunkData) {
		t.Fatalf("garbage: err = %v, want ErrBadChunkData", err)
	}
	compressed := deflate(t, pattern(12288))
	compressed[len(compressed)-1] ^= 0xff
	badChecksum := &ChunkData{PrimaryBitmask: 1, Compressed: compressed}
	if _, err := badChecksum.Inflate(); !errors.Is(err, ErrBadChunkData) {
		t.Fatalf("wrong checksum: err = %v, want ErrBadChunkData", err)
	}
}

func TestMapChunkBulkInflateSplitsColumns(t *testing.T) {
	columns := []ChunkColumn{
		{ChunkX: 0, ChunkZ: 0, PrimaryBitmask: 0x0003, AddBitmask: 0x0001},
		{ChunkX: -1, ChunkZ: 4, PrimaryBitmask: 0x0001},
	}
	for _, tc := range []struct {
		skyLight bool
		sizes    []int
	}{
		// Two sections with block ids, metadata and block light, biomes, one add array, and
		// sky light for both sections; then one section without an add array.
		{true, []int{2*8192 + 256 + 2048 + 2*2048, 8192 + 256 + 2048}},
		{false, []int{2*8192 + 256 + 2048, 8192 + 256}},
	} {
		raw := pattern(tc.sizes[0] + tc.sizes[1])
		p := &MapChunkBulk{SkyLight: tc.skyLight, Compressed: deflate(t, raw), Columns: columns}
		for i, column := range columns {
			if size := column.InflatedSize(tc.skyLight); size != tc.sizes[i] {
				t.Errorf("skyLight=%v column %d: InflatedSize = %d, want %d", tc.skyLight, i, size, tc.sizes[i])
			}
		}
		got, err := p.Inflate()
		if err != nil {
			t.Fatalf("skyLight=%v: %v", tc.skyLight, err)
		}
		want := [][]byte{raw[:tc.sizes[0]], raw[tc.sizes[0]:]}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("skyLight=%v: columns of %d and %d bytes do not match the stream", tc.skyLight, len(got[0]), len(got[1]))
		}
	}
}

func TestMapChunkBulkInflateZeroFillsMissingColumns(t *testing.T) {
	column := ChunkColumn{PrimaryBitmask: 0x0001}
	size := column.InflatedSize(true)
	raw := pattern(size + 100)
	p := &MapChunkBulk{SkyLight: true, Compressed: deflate(t, raw), Columns: []ChunkColumn{column, column, column}}
	got, err := p.Inflate()
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got[0], raw[:size]) {
		t.Error("first column does not match the stream")
	}
	if !bytes.Equal(got[1][:100], raw[size:]) || !bytes.Equal(got[1][100:], make([]byte, size-100)) {
		t.Error("second column must hold the stream's tail followed by zeros")
	}
	if !bytes.Equal(got[2], make([]byte, size)) {
		t.Error("third column must be zero")
	}
}

func TestMapChunkBulkInflateRejectsWhatTheClientCannotBuffer(t *testing.T) {
	// 10909 columns are the first count whose reserved size overflows a signed 32-bit integer.
	p := &MapChunkBulk{Columns: make([]ChunkColumn, 10909)}
	if _, err := p.Inflate(); !errors.Is(err, ErrBadChunkData) {
		t.Fatalf("10909 columns: err = %v, want ErrBadChunkData", err)
	}
	corrupt := &MapChunkBulk{Compressed: []byte{0xde, 0xad, 0xbe, 0xef}, Columns: make([]ChunkColumn, 1)}
	if _, err := corrupt.Inflate(); !errors.Is(err, ErrBadChunkData) {
		t.Fatalf("garbage: err = %v, want ErrBadChunkData", err)
	}
}

func TestMultiBlockChangeRecords(t *testing.T) {
	records := []BlockChangeRecord{
		{X: 3, Z: 7, Y: 200, BlockID: 1234, Metadata: 9},
		{X: 15, Z: 0, Y: 0, BlockID: 4095, Metadata: 15},
	}
	p := NewMultiBlockChange(2, -2, records)
	want := []byte{0x37, 0xc8, 0x4d, 0x29, 0xf0, 0x00, 0xff, 0xff}
	if !bytes.Equal(p.Data, want) || p.RecordCount != 2 {
		t.Fatalf("records packed as % x (count %d), want % x", p.Data, p.RecordCount, want)
	}
	if got := p.Records(); !reflect.DeepEqual(got, records) {
		t.Fatalf("Records = %+v, want %+v", got, records)
	}

	// The client stops quietly where the data runs out and ignores records past the count.
	p.RecordCount = 5
	if got := p.Records(); !reflect.DeepEqual(got, records) {
		t.Fatalf("count beyond data: Records = %+v", got)
	}
	p.RecordCount = 1
	if got := p.Records(); !reflect.DeepEqual(got, records[:1]) {
		t.Fatalf("count below data: Records = %+v", got)
	}
	p.Data = p.Data[:6]
	p.RecordCount = 2
	if got := p.Records(); !reflect.DeepEqual(got, records[:1]) {
		t.Fatalf("partial trailing record: Records = %+v", got)
	}
}

func TestExplosionAffectedBlocks(t *testing.T) {
	p := &Explosion{X: -1.5, Y: 64.9, Z: 8.25, Records: []ExplosionRecord{{DX: 1, DY: -1, DZ: 0}, {DX: -128, DY: 127, DZ: 3}}}
	want := []Position{{X: 0, Y: 63, Z: 8}, {X: -129, Y: 191, Z: 11}}
	if got := p.AffectedBlocks(); !reflect.DeepEqual(got, want) {
		t.Fatalf("AffectedBlocks = %+v, want %+v", got, want)
	}
}
