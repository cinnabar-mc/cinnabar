// Package translate joins a Java Edition 1.7.10 server and presents it as a Bedrock session:
// packets from the server become Bedrock packets for the client, and the client's Bedrock
// packets become the packets a 1.7.10 client sends.
package translate

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/javaedition/conn"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Options configures a join.
type Options struct {
	// Username is the name sent in the login. Online-mode servers ignore it in favour of the
	// authenticated profile.
	Username string
	// JoinSession registers the join with the session service for online-mode servers. A nil
	// JoinSession can only join offline-mode servers.
	JoinSession func(ctx context.Context, serverHash string) error
	// Dialer opens the TCP connection; nil uses the system dialer.
	Dialer func(ctx context.Context, network, address string) (net.Conn, error)
	// Resolver looks up the server's SRV record; nil uses the system resolver.
	Resolver conn.Resolver
	// Logger receives skipped and malformed packet reports; nil discards them.
	Logger *slog.Logger
}

// outbound is one Bedrock packet queued for the client, kept decoded for observers.
type outbound struct {
	packet packet.Packet
	data   []byte
}

// Session is a joined Java server seen as a Bedrock upstream. One goroutine may read batches
// while another writes packets.
type Session struct {
	conn *conn.Conn
	log  *slog.Logger

	// batches carries translated packets to ReadBatchRaw. pending is the batch being built by
	// whichever direction is translating; pendingMu guards it.
	batches   chan []outbound
	pendingMu sync.Mutex
	pending   []outbound

	closeOnce sync.Once
	closed    chan struct{}
	failure   atomic.Pointer[error]

	// mu guards state, which both directions read and update.
	mu    sync.Mutex
	state state

	latency atomic.Int64
}

// Dial joins the server at address and starts translating. The returned session's first
// batch ends with StartGame, as a Bedrock login does.
func Dial(ctx context.Context, address string, options Options) (*Session, error) {
	logger := options.Logger
	if logger == nil {
		logger = slog.New(slog.DiscardHandler)
	}
	connection, err := conn.Dial(ctx, address, conn.Options{
		Username:    options.Username,
		JoinSession: options.JoinSession,
		Dialer:      options.Dialer,
		Resolver:    options.Resolver,
	})
	if err != nil {
		return nil, err
	}
	session := &Session{
		conn:    connection,
		log:     logger,
		batches: make(chan []outbound, 64),
		closed:  make(chan struct{}),
		state:   newState(),
	}
	go session.read()
	return session, nil
}

// read translates server packets until the connection ends, then reports why.
func (s *Session) read() {
	for {
		pk, err := s.conn.ReadPacket()
		if err != nil {
			s.fail(err)
			return
		}
		s.mu.Lock()
		err = s.handleJava(pk)
		s.mu.Unlock()
		if err != nil {
			s.fail(err)
			return
		}
		if !s.flushPending() {
			return
		}
	}
}

// queue appends Bedrock packets to the batch the packet being translated produces.
func (s *Session) queue(packets ...packet.Packet) {
	s.pendingMu.Lock()
	defer s.pendingMu.Unlock()
	for _, pk := range packets {
		s.pending = append(s.pending, outbound{packet: pk, data: encodeBedrock(pk)})
	}
}

// flushPending hands the current batch to the reader; it reports false once the session
// has closed.
func (s *Session) flushPending() bool {
	s.pendingMu.Lock()
	batch := s.pending
	s.pending = nil
	s.pendingMu.Unlock()
	if len(batch) == 0 {
		return true
	}
	select {
	case s.batches <- batch:
		return true
	case <-s.closed:
		return false
	}
}

// fail records the first reason the session ended and closes it.
func (s *Session) fail(err error) {
	s.failure.CompareAndSwap(nil, &err)
	_ = s.Close()
}

// err returns why the session ended, or net.ErrClosed when it was closed locally.
func (s *Session) err() error {
	if failure := s.failure.Load(); failure != nil {
		return *failure
	}
	return net.ErrClosed
}

// ReadBatchRaw returns the Bedrock packets one server packet translated to. Packets whose id
// decode selects carry their decoded form too.
func (s *Session) ReadBatchRaw(decode func(id uint32) bool) ([]minecraft.RawPacket, error) {
	var batch []outbound
	select {
	case batch = <-s.batches:
	case <-s.closed:
		// Deliver what was translated before the session ended, such as a final disconnect.
		select {
		case batch = <-s.batches:
		default:
			var disconnect *conn.DisconnectError
			if err := s.err(); errors.As(err, &disconnect) {
				return nil, minecraft.DisconnectError(chatText(disconnect.Reason))
			}
			return nil, s.err()
		}
	}
	packets := make([]minecraft.RawPacket, len(batch))
	for i, entry := range batch {
		packets[i] = minecraft.RawPacket{ID: entry.packet.ID(), Data: entry.data}
		if decode != nil && decode(entry.packet.ID()) {
			packets[i].Decoded = []packet.Packet{entry.packet}
		}
	}
	return packets, nil
}

// WritePacketRaw translates one encoded Bedrock client packet and buffers the Java packets
// it becomes.
func (s *Session) WritePacketRaw(data []byte) error {
	pk, err := decodeBedrock(data)
	if err != nil {
		// A packet this side cannot decode has no Java counterpart to send.
		s.log.Debug("skipping undecodable client packet", "error", err)
		return nil
	}
	if pk == nil {
		return nil
	}
	return s.WritePacket(pk)
}

// WritePacket translates one Bedrock client packet and buffers the Java packets it becomes.
func (s *Session) WritePacket(pk packet.Packet) error {
	select {
	case <-s.closed:
		return s.err()
	default:
	}
	s.mu.Lock()
	err := s.handleBedrock(pk)
	s.mu.Unlock()
	// Replies to the client, such as the granted chunk radius, travel like translated packets.
	s.flushPending()
	return err
}

// WritePacketImmediate translates the packets and flushes them at once.
func (s *Session) WritePacketImmediate(packets ...packet.Packet) error {
	for _, pk := range packets {
		if err := s.WritePacket(pk); err != nil {
			return err
		}
	}
	return s.Flush()
}

// send buffers Java packets for the server.
func (s *Session) send(packets ...java.Packet) error {
	for _, pk := range packets {
		if err := s.conn.WritePacket(pk); err != nil {
			return err
		}
	}
	return nil
}

// Flush sends the buffered Java packets.
func (s *Session) Flush() error {
	return s.conn.Flush()
}

// Close ends the session; pending reads return its error.
func (s *Session) Close() error {
	var err error
	s.closeOnce.Do(func() {
		close(s.closed)
		err = s.conn.Close()
	})
	return err
}

// Abort closes the session without flushing.
func (s *Session) Abort() error {
	return s.Close()
}

// IdentityData returns the profile the server logged the player in as.
func (s *Session) IdentityData() login.IdentityData {
	profile := s.conn.Profile()
	return login.IdentityData{Identity: profile.UUID, DisplayName: profile.Username}
}

// ResourcePacks is empty: 1.7.10 servers offer no Bedrock packs.
func (s *Session) ResourcePacks() []*resource.Pack {
	return nil
}

// TexturePacksRequired is false: there are no packs to require.
func (s *Session) TexturePacksRequired() bool {
	return false
}

// GameData returns the facts StartGame announced, once the server has sent its join packet.
func (s *Session) GameData() minecraft.GameData {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.state.gameData
}

// Latency is half the round trip the server last reported for the player in its player
// list, or zero before it has.
func (s *Session) Latency() time.Duration {
	return time.Duration(s.latency.Load())
}
