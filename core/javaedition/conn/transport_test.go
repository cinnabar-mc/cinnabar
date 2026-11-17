package conn

import (
	"bytes"
	"crypto/aes"
	"errors"
	"io"
	"net"
	"os"
	"testing"
	"time"
)

// pipe returns a Transport and the raw peer end of an in-memory stream.
func pipe(t *testing.T) (*Transport, net.Conn) {
	t.Helper()
	local, peer := net.Pipe()
	t.Cleanup(func() {
		_ = local.Close()
		_ = peer.Close()
	})
	return NewTransport(local, 0, 0), peer
}

// feed writes raw to the peer end and then closes it, from a goroutine because the stream is
// unbuffered.
func feed(peer net.Conn, raw []byte) {
	go func() {
		_, _ = peer.Write(raw)
		_ = peer.Close()
	}()
}

func TestReadFrame(t *testing.T) {
	big := bytes.Repeat([]byte{0xab}, 300)
	transport, peer := pipe(t)
	// A one-byte frame, an empty frame that must be skipped, and a frame with a two-byte length.
	feed(peer, append(append([]byte{0x01, 0x42, 0x00, 0xac, 0x02}, big...), 0x02, 0x01))
	for _, want := range [][]byte{{0x42}, big} {
		got, err := transport.ReadFrame()
		if err != nil || !bytes.Equal(got, want) {
			t.Fatalf("ReadFrame = %d bytes, err %v; want %d bytes", len(got), err, len(want))
		}
	}
	if _, err := transport.ReadFrame(); !errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("frame cut short: err = %v, want unexpected EOF", err)
	}
}

func TestReadFrameCleanEOF(t *testing.T) {
	transport, peer := pipe(t)
	feed(peer, nil)
	if _, err := transport.ReadFrame(); !errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		t.Fatalf("err = %v, want EOF", err)
	}
}

func TestReadFrameRejectsLengthWiderThan21Bits(t *testing.T) {
	transport, peer := pipe(t)
	feed(peer, []byte{0x80, 0x80, 0x80, 0x01})
	if _, err := transport.ReadFrame(); !errors.Is(err, ErrFrameTooLarge) {
		t.Fatalf("err = %v, want ErrFrameTooLarge", err)
	}
}

func TestReadFrameAcceptsLargestLength(t *testing.T) {
	transport, peer := pipe(t)
	payload := bytes.Repeat([]byte{0x5a}, MaxFrameSize)
	feed(peer, append([]byte{0xff, 0xff, 0x7f}, payload...))
	got, err := transport.ReadFrame()
	if err != nil || !bytes.Equal(got, payload) {
		t.Fatalf("ReadFrame = %d bytes, err %v; want %d bytes", len(got), err, MaxFrameSize)
	}
}

func TestWriteFrame(t *testing.T) {
	transport, peer := pipe(t)
	if err := transport.WriteFrame(make([]byte, MaxFrameSize+1)); !errors.Is(err, ErrFrameTooLarge) {
		t.Fatalf("oversized frame: err = %v, want ErrFrameTooLarge", err)
	}
	if err := transport.WriteFrame([]byte{0x01, 0x02}); err != nil {
		t.Fatal(err)
	}
	if err := transport.WriteFrame(bytes.Repeat([]byte{0x03}, 128)); err != nil {
		t.Fatal(err)
	}
	flushed := make(chan error, 1)
	go func() { flushed <- transport.Flush() }()
	want := append([]byte{0x02, 0x01, 0x02, 0x80, 0x01}, bytes.Repeat([]byte{0x03}, 128)...)
	got := make([]byte, len(want))
	if _, err := io.ReadFull(peer, got); err != nil || !bytes.Equal(got, want) {
		t.Fatalf("peer read % x, err %v", got[:8], err)
	}
	if err := <-flushed; err != nil {
		t.Fatal(err)
	}
}

func TestEncryptedFramesRoundTrip(t *testing.T) {
	left, right := net.Pipe()
	t.Cleanup(func() {
		_ = left.Close()
		_ = right.Close()
	})
	client, server := NewTransport(left, 0, 0), NewTransport(right, 0, 0)
	secret := testBytes(16, 9)
	if err := client.EnableEncryption(secret); err != nil {
		t.Fatal(err)
	}
	if err := server.EnableEncryption(secret); err != nil {
		t.Fatal(err)
	}
	if err := client.EnableEncryption(secret); err == nil {
		t.Fatal("encryption was enabled twice")
	}
	frames := [][]byte{{0x00, 0x01}, testBytes(5000, 10), {0x7f}}
	go func() {
		for _, frame := range frames {
			_ = client.WriteFrame(frame)
		}
		_ = client.Flush()
	}()
	for _, want := range frames {
		got, err := server.ReadFrame()
		if err != nil || !bytes.Equal(got, want) {
			t.Fatalf("ReadFrame = %d bytes, err %v; want %d bytes", len(got), err, len(want))
		}
	}
}

func TestEncryptionReachesTheWire(t *testing.T) {
	transport, peer := pipe(t)
	secret := testBytes(16, 11)
	if err := transport.EnableEncryption(secret); err != nil {
		t.Fatal(err)
	}
	if err := transport.WriteFrame([]byte{0x01, 0x02, 0x03}); err != nil {
		t.Fatal(err)
	}
	go func() { _ = transport.Flush() }()
	got := make([]byte, 4)
	if _, err := io.ReadFull(peer, got); err != nil {
		t.Fatal(err)
	}
	block, err := aes.NewCipher(secret)
	if err != nil {
		t.Fatal(err)
	}
	if want := referenceCFB8(block, secret, []byte{0x03, 0x01, 0x02, 0x03}, false); !bytes.Equal(got, want) {
		t.Fatalf("wire bytes % x, want the frame encrypted as % x", got, want)
	}

	// Bytes from the peer are decrypted with the same key, starting from the same vector.
	go func() { _, _ = peer.Write(referenceCFB8(block, secret, []byte{0x02, 0x0a, 0x0b}, false)) }()
	frame, err := transport.ReadFrame()
	if err != nil || !bytes.Equal(frame, []byte{0x0a, 0x0b}) {
		t.Fatalf("ReadFrame = % x, err %v", frame, err)
	}
}

func TestReadFrameTimesOutWhenPeerIsSilent(t *testing.T) {
	local, peer := net.Pipe()
	t.Cleanup(func() {
		_ = local.Close()
		_ = peer.Close()
	})
	transport := NewTransport(local, time.Millisecond, 0)
	if _, err := transport.ReadFrame(); !errors.Is(err, os.ErrDeadlineExceeded) {
		t.Fatalf("err = %v, want a deadline error", err)
	}
}

func TestReadTimeoutOption(t *testing.T) {
	for configured, want := range map[time.Duration]time.Duration{
		0:               DefaultReadTimeout,
		-1:              0,
		5 * time.Second: 5 * time.Second,
	} {
		if got := readTimeout(Options{ReadTimeout: configured}); got != want {
			t.Errorf("ReadTimeout %v gives %v, want %v", configured, got, want)
		}
	}
}
