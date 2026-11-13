package protocol

import (
	"bytes"
	"compress/zlib"
	"errors"
	"fmt"
	"io"
	"math"
	"math/bits"
)

// Sizes of the pieces of an inflated chunk column. A 16-block-high section has one byte per
// block for the low id bits and one nibble per block for each of metadata, block light, sky
// light and the high id bits.
const (
	chunkSectionBlockBytes  = 4096
	chunkSectionNibbleBytes = 2048
	chunkBiomeBytes         = 256
	// chunkDataSectionBytes is what the client reserves per section present in a ChunkData.
	chunkDataSectionBytes = 12288
	// chunkBulkColumnBytes is what the client reserves per column of a MapChunkBulk: sixteen
	// sections with every array, plus biomes.
	chunkBulkColumnBytes = 196864
)

// ErrBadChunkData reports chunk bytes that do not inflate. 1.7.10 drops the connection when
// it reads such a packet.
var ErrBadChunkData = errors.New("protocol: bad compressed chunk data")

// ChunkData carries the sections of one chunk column selected by PrimaryBitmask, bit n for
// the section at height 16n. Compressed is the zlib stream as sent; Inflate expands it. A
// column sent with GroundUpContinuous and an empty PrimaryBitmask is unloaded by the client.
//
// The inflated bytes hold, for the selected sections in ascending order: all low block id
// arrays, then all metadata arrays, all block light arrays, all sky light arrays when the
// dimension has a sky, the high block id arrays for sections in AddBitmask, and the biome
// array when GroundUpContinuous is set.
type ChunkData struct {
	playClientbound
	ChunkX, ChunkZ     int32
	GroundUpContinuous bool
	PrimaryBitmask     uint16
	AddBitmask         uint16
	Compressed         []byte
}

func (*ChunkData) ID() int32 { return 0x21 }

func (p *ChunkData) Marshal(w *Writer) {
	w.Int32(p.ChunkX)
	w.Int32(p.ChunkZ)
	w.Bool(p.GroundUpContinuous)
	w.Uint16(p.PrimaryBitmask)
	w.Uint16(p.AddBitmask)
	w.Int32(int32(w.length(len(p.Compressed), math.MaxInt32, "chunk bytes")))
	w.Raw(p.Compressed)
}

// Unmarshal does not inflate Compressed; a stream 1.7.10 would reject while reading is
// reported by Inflate instead.
func (p *ChunkData) Unmarshal(r *Reader) error {
	p.ChunkX = r.Int32()
	p.ChunkZ = r.Int32()
	p.GroundUpContinuous = r.Bool()
	p.PrimaryBitmask = r.Uint16()
	p.AddBitmask = r.Uint16()
	p.Compressed = r.Bytes(r.count(int(r.Int32()), "chunk bytes"))
	return r.Err()
}

// InflatedSize returns the size of the buffer the client inflates into: a fixed amount per
// section in PrimaryBitmask, plus the biome array when GroundUpContinuous is set.
func (p *ChunkData) InflatedSize() int {
	size := chunkDataSectionBytes * bits.OnesCount16(p.PrimaryBitmask)
	if p.GroundUpContinuous {
		size += chunkBiomeBytes
	}
	return size
}

// Inflate expands Compressed into a buffer of InflatedSize bytes, as the client does. A
// stream that yields fewer bytes leaves the rest zero and one that yields more is cut off;
// only corrupt data is an error.
func (p *ChunkData) Inflate() ([]byte, error) {
	size := p.InflatedSize()
	data, err := inflateChunk(p.Compressed, size, size)
	if err != nil {
		return nil, err
	}
	return append(data, make([]byte, size-len(data))...), nil
}

// ChunkColumn locates one column of a MapChunkBulk and says which sections it carries.
type ChunkColumn struct {
	ChunkX, ChunkZ int32
	PrimaryBitmask uint16
	AddBitmask     uint16
}

// InflatedSize returns how many inflated bytes the client assigns to the column: block ids,
// metadata and block light for each section in PrimaryBitmask, sky light for those sections
// when skyLight is set, high block id bits for each section in AddBitmask, and biomes.
func (c ChunkColumn) InflatedSize(skyLight bool) int {
	sections := bits.OnesCount16(c.PrimaryBitmask)
	size := (chunkSectionBlockBytes+2*chunkSectionNibbleBytes)*sections + chunkBiomeBytes
	size += chunkSectionNibbleBytes * bits.OnesCount16(c.AddBitmask)
	if skyLight {
		size += chunkSectionNibbleBytes * sections
	}
	return size
}

// MapChunkBulk carries several whole chunk columns in one zlib stream. Every column is sent
// ground-up with biomes, laid out as described on ChunkData; SkyLight says whether the
// columns include sky light arrays.
type MapChunkBulk struct {
	playClientbound
	SkyLight   bool
	Compressed []byte
	Columns    []ChunkColumn
}

func (*MapChunkBulk) ID() int32 { return 0x26 }

func (p *MapChunkBulk) Marshal(w *Writer) {
	w.Int16(int16(w.length(len(p.Columns), math.MaxInt16, "chunk columns")))
	w.Int32(int32(w.length(len(p.Compressed), math.MaxInt32, "chunk bytes")))
	w.Bool(p.SkyLight)
	w.Raw(p.Compressed)
	for _, column := range p.Columns {
		w.Int32(column.ChunkX)
		w.Int32(column.ChunkZ)
		w.Uint16(column.PrimaryBitmask)
		w.Uint16(column.AddBitmask)
	}
}

// Unmarshal does not inflate Compressed; a stream 1.7.10 would reject while reading is
// reported by Inflate instead.
func (p *MapChunkBulk) Unmarshal(r *Reader) error {
	count := r.count(int(r.Int16()), "chunk columns")
	size := r.count(int(r.Int32()), "chunk bytes")
	p.SkyLight = r.Bool()
	p.Compressed = r.Bytes(size)
	p.Columns = nil
	if count > 0 {
		p.Columns = make([]ChunkColumn, 0, r.capacity(count, 12))
	}
	for i := 0; i < count && r.Err() == nil; i++ {
		p.Columns = append(p.Columns, ChunkColumn{
			ChunkX:         r.Int32(),
			ChunkZ:         r.Int32(),
			PrimaryBitmask: r.Uint16(),
			AddBitmask:     r.Uint16(),
		})
	}
	return r.Err()
}

// Inflate expands Compressed and splits it into one slice per column, each
// ChunkColumn.InflatedSize bytes long, as the client does. Bytes the stream does not provide
// read as zero. The slices may share memory and must be treated as read-only. Corrupt data,
// or a column count whose reserved buffer the client could not allocate, is an error.
func (p *MapChunkBulk) Inflate() ([][]byte, error) {
	// The client reserves a fixed amount per column in a 32-bit size that may wrap.
	reserved := int32(uint32(chunkBulkColumnBytes) * uint32(len(p.Columns)))
	if reserved < 0 {
		return nil, fmt.Errorf("%w: %d columns overflow the client's buffer", ErrBadChunkData, len(p.Columns))
	}
	sizes := make([]int, len(p.Columns))
	total := 0
	for i, column := range p.Columns {
		sizes[i] = column.InflatedSize(p.SkyLight)
		total += sizes[i]
		if total > int(reserved) {
			return nil, fmt.Errorf("%w: %d columns overflow the client's buffer", ErrBadChunkData, len(p.Columns))
		}
	}
	data, err := inflateChunk(p.Compressed, total, int(reserved))
	if err != nil {
		return nil, err
	}
	// Columns past the end of the stream are all zero; one shared block serves them without
	// allocating what a hostile column count asks for.
	var zeros []byte
	columns := make([][]byte, len(p.Columns))
	offset := 0
	for i, size := range sizes {
		switch {
		case offset+size <= len(data):
			columns[i] = data[offset : offset+size : offset+size]
		case offset < len(data):
			columns[i] = make([]byte, size)
			copy(columns[i], data[offset:])
		default:
			if zeros == nil {
				zeros = make([]byte, chunkBulkColumnBytes)
			}
			columns[i] = zeros[:size:size]
		}
		offset += size
	}
	return columns, nil
}

// Each inflates the columns one at a time and passes each to visit, in the packet's order.
// The data slice is ChunkColumn.InflatedSize bytes long and is reused for the next column,
// so visit must not keep it. Bytes the stream does not provide read as zero, as in Inflate,
// which this replaces where holding every column at once is too much: the working memory is
// one column whatever the column count. Corrupt data within the columns, or a column count
// whose reserved buffer the client could not allocate or would overrun, is an error. Unlike
// Inflate, it does not look for damage after the last column.
func (p *MapChunkBulk) Each(visit func(column ChunkColumn, data []byte) error) error {
	// The client reserves a fixed amount per column in a 32-bit size that may wrap.
	reserved := int32(uint32(chunkBulkColumnBytes) * uint32(len(p.Columns)))
	if reserved < 0 {
		return fmt.Errorf("%w: %d columns overflow the client's buffer", ErrBadChunkData, len(p.Columns))
	}
	total := 0
	for _, column := range p.Columns {
		if total += column.InflatedSize(p.SkyLight); total > int(reserved) {
			return fmt.Errorf("%w: %d columns overflow the client's buffer", ErrBadChunkData, len(p.Columns))
		}
	}
	var stream io.Reader
	if zr, err := zlib.NewReader(bytes.NewReader(p.Compressed)); err == nil {
		stream = zr
	} else if len(p.Compressed) >= 2 {
		return fmt.Errorf("%w: %v", ErrBadChunkData, err)
	}
	buffer := make([]byte, chunkBulkColumnBytes)
	for _, column := range p.Columns {
		data := buffer[:column.InflatedSize(p.SkyLight)]
		clear(data)
		if stream != nil {
			if _, err := io.ReadFull(stream, data); err != nil {
				if !endsEarly(err) {
					return fmt.Errorf("%w: %v", ErrBadChunkData, err)
				}
				// The stream is spent: this column keeps what it got and the rest are zero.
				stream = nil
			}
		}
		if err := visit(column, data); err != nil {
			return err
		}
	}
	return nil
}

// inflateChunk mirrors the client's one inflate call into a buffer of reserved bytes and
// returns the first keep bytes of output, fewer when the stream ends sooner. Output past
// reserved is never looked at. A stream that merely ends early is not an error; corrupt data
// within the client's reach is ErrBadChunkData.
func inflateChunk(compressed []byte, keep, reserved int) ([]byte, error) {
	zr, err := zlib.NewReader(bytes.NewReader(compressed))
	if err != nil {
		if len(compressed) < 2 {
			// Too short to hold a stream header: nothing inflates and nothing is wrong yet.
			return nil, nil
		}
		return nil, fmt.Errorf("%w: %v", ErrBadChunkData, err)
	}
	data, err := io.ReadAll(io.LimitReader(zr, int64(keep)))
	if err != nil {
		if endsEarly(err) {
			return data, nil
		}
		return nil, fmt.Errorf("%w: %v", ErrBadChunkData, err)
	}
	if len(data) < keep {
		return data, nil
	}
	// Check the rest of what the client would inflate, and one byte more: damage that follows
	// a full buffer without producing output still fails the client's call.
	if _, err := io.CopyN(io.Discard, zr, int64(reserved-keep)+1); err != nil && !endsEarly(err) {
		return nil, fmt.Errorf("%w: %v", ErrBadChunkData, err)
	}
	return data, nil
}

// endsEarly reports whether err only says a stream stopped, cleanly or not.
func endsEarly(err error) bool {
	return errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF)
}
