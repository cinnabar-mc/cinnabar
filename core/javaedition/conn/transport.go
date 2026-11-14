package conn

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"net"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

const (
	// maxFrameLengthBytes is how many bytes the VarInt frame length may take.
	maxFrameLengthBytes = 3
	// MaxFrameSize is the largest frame payload the 21-bit length prefix can describe.
	MaxFrameSize = 1<<(7*maxFrameLengthBytes) - 1
	// readBufferSize is the size of the buffer frames are read through.
	readBufferSize = 64 << 10
)

// ErrFrameTooLarge reports a frame whose length does not fit in three VarInt bytes. 1.7.10
// neither reads nor writes such a frame.
var ErrFrameTooLarge = errors.New("conn: frame length wider than 21-bit")

// Transport carries length-prefixed frames over a stream, optionally through the AES/CFB8
// cipher. It serves either end of a connection. One goroutine may read while others write:
// ReadFrame must not be called concurrently with itself, WriteFrame and Flush may be.
type Transport struct {
	conn         net.Conn
	readTimeout  time.Duration
	writeTimeout time.Duration

	reader  *bufio.Reader
	decrypt *cfb8

	writeMu sync.Mutex
	pending []byte
	encrypt *cfb8
}

// NewTransport wraps conn. A positive readTimeout fails a read once no byte has arrived for
// that long, and a positive writeTimeout bounds each Flush.
func NewTransport(conn net.Conn, readTimeout, writeTimeout time.Duration) *Transport {
	t := &Transport{conn: conn, readTimeout: readTimeout, writeTimeout: writeTimeout}
	t.reader = bufio.NewReaderSize(transportReader{t}, readBufferSize)
	return t
}

// transportReader reads from the stream for the Transport's buffer, renewing the idle
// deadline and decrypting once encryption is on.
type transportReader struct{ t *Transport }

// Read implements io.Reader.
func (r transportReader) Read(p []byte) (int, error) {
	t := r.t
	if t.readTimeout > 0 {
		if err := t.conn.SetReadDeadline(time.Now().Add(t.readTimeout)); err != nil {
			return 0, err
		}
	}
	n, err := t.conn.Read(p)
	if t.decrypt != nil {
		t.decrypt.XORKeyStream(p[:n], p[:n])
	}
	return n, err
}

// EnableEncryption turns on AES/CFB8 in both directions with sharedSecret as key and initial
// vector. Frames already queued by WriteFrame stay as they are, so callers flush first. Bytes
// the peer sent before it switched must have been read already. It must not run concurrently
// with ReadFrame.
func (t *Transport) EnableEncryption(sharedSecret []byte) error {
	encrypt, decrypt, err := newStreamCiphers(sharedSecret)
	if err != nil {
		return err
	}
	t.writeMu.Lock()
	defer t.writeMu.Unlock()
	if t.encrypt != nil {
		return errors.New("conn: encryption is already enabled")
	}
	t.encrypt, t.decrypt = encrypt, decrypt
	return nil
}

// ReadFrame returns the payload of the next frame: the packet id followed by the packet body.
// Empty frames are skipped, as 1.7.10 skips them. A length prefix longer than three bytes is
// ErrFrameTooLarge.
func (t *Transport) ReadFrame() ([]byte, error) {
	for {
		length, err := t.readFrameLength()
		if err != nil {
			return nil, err
		}
		if length == 0 {
			continue
		}
		payload := make([]byte, length)
		if _, err := io.ReadFull(t.reader, payload); err != nil {
			if errors.Is(err, io.EOF) {
				err = io.ErrUnexpectedEOF
			}
			return nil, fmt.Errorf("read frame of %d bytes: %w", length, err)
		}
		return payload, nil
	}
}

// readFrameLength reads the VarInt length prefix of a frame.
func (t *Transport) readFrameLength() (int, error) {
	length := 0
	for i := 0; i < maxFrameLengthBytes; i++ {
		b, err := t.reader.ReadByte()
		if err != nil {
			if i > 0 && errors.Is(err, io.EOF) {
				err = io.ErrUnexpectedEOF
			}
			return 0, fmt.Errorf("read frame length: %w", err)
		}
		length |= int(b&0x7f) << (7 * i)
		if b&0x80 == 0 {
			return length, nil
		}
	}
	return 0, ErrFrameTooLarge
}

// WriteFrame queues one frame holding payload, encrypting it when encryption is on. Nothing
// reaches the peer before Flush. A payload longer than MaxFrameSize is ErrFrameTooLarge.
func (t *Transport) WriteFrame(payload []byte) error {
	if len(payload) > MaxFrameSize {
		return fmt.Errorf("write frame of %d bytes: %w", len(payload), ErrFrameTooLarge)
	}
	t.writeMu.Lock()
	defer t.writeMu.Unlock()
	start := len(t.pending)
	t.pending = protocol.AppendVarInt(t.pending, int32(len(payload)))
	t.pending = append(t.pending, payload...)
	if t.encrypt != nil {
		t.encrypt.XORKeyStream(t.pending[start:], t.pending[start:])
	}
	return nil
}

// Flush sends every queued frame.
func (t *Transport) Flush() error {
	t.writeMu.Lock()
	defer t.writeMu.Unlock()
	if len(t.pending) == 0 {
		return nil
	}
	if t.writeTimeout > 0 {
		if err := t.conn.SetWriteDeadline(time.Now().Add(t.writeTimeout)); err != nil {
			return fmt.Errorf("flush frames: %w", err)
		}
	}
	_, err := t.conn.Write(t.pending)
	t.pending = t.pending[:0]
	if err != nil {
		return fmt.Errorf("flush frames: %w", err)
	}
	return nil
}

// Close closes the underlying stream. Blocked reads and writes return an error.
func (t *Transport) Close() error { return t.conn.Close() }

// LocalAddr returns the local network address.
func (t *Transport) LocalAddr() net.Addr { return t.conn.LocalAddr() }

// RemoteAddr returns the peer's network address.
func (t *Transport) RemoteAddr() net.Addr { return t.conn.RemoteAddr() }
