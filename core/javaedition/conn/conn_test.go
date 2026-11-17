package conn_test

import (
	"context"
	"errors"
	"io"
	"net"
	"reflect"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/conn"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

// testContext returns a context that ends when the test does or after the shared test
// timeout, whichever comes first.
func testContext(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), testwait.DefaultTimeout)
	t.Cleanup(cancel)
	return ctx
}

// startServer runs a fake server for the length of the test.
func startServer(t *testing.T, config javatest.Config) *javatest.Server {
	t.Helper()
	server, err := javatest.Start(config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	return server
}

// join dials server with opts and returns both ends of the logged-in connection.
func join(t *testing.T, server *javatest.Server, opts conn.Options) (*conn.Conn, *javatest.Client) {
	t.Helper()
	ctx := testContext(t)
	client, err := conn.Dial(ctx, server.Addr(), opts)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	t.Cleanup(func() { _ = client.Close() })
	peer, err := server.Accept(ctx)
	if err != nil {
		t.Fatalf("Accept: %v", err)
	}
	return client, peer
}

// exchange sends one packet each way and checks both arrive intact.
func exchange(t *testing.T, client *conn.Conn, peer *javatest.Client) {
	t.Helper()
	down := &protocol.JoinGame{EntityID: 7, GameMode: 1, Dimension: -1, Difficulty: 2, MaxPlayers: 20, LevelType: "default"}
	if err := peer.Write(down); err != nil {
		t.Fatal(err)
	}
	if got, err := client.ReadPacket(); err != nil || !reflect.DeepEqual(got, down) {
		t.Fatalf("client read %+v, err %v; want %+v", got, err, down)
	}
	up := []protocol.Packet{
		&protocol.ClientSettings{Locale: "en_US", ViewDistance: 8, ChatColors: true, Difficulty: 2, ShowCape: true},
		&protocol.ClientPluginMessage{Channel: "MC|Brand", Data: []byte("vanilla")},
	}
	for _, p := range up {
		if err := client.WritePacket(p); err != nil {
			t.Fatal(err)
		}
	}
	if err := client.Flush(); err != nil {
		t.Fatal(err)
	}
	for _, want := range up {
		if got, err := peer.Read(); err != nil || !reflect.DeepEqual(got, want) {
			t.Fatalf("server read %+v, err %v; want %+v", got, err, want)
		}
	}
}

func TestOfflineLogin(t *testing.T) {
	server := startServer(t, javatest.Config{})
	client, peer := join(t, server, conn.Options{Username: "Steve"})

	host, port, err := net.SplitHostPort(server.Addr())
	if err != nil {
		t.Fatal(err)
	}
	if got := client.Address().String(); got != server.Addr() {
		t.Errorf("Address = %s, want %s", got, server.Addr())
	}
	wantHandshake := protocol.Handshake{
		ProtocolVersion: 5, ServerAddress: host, ServerPort: client.Address().Port, NextState: protocol.StateLogin,
	}
	if peer.Handshake != wantHandshake || net.JoinHostPort(host, port) != server.Addr() {
		t.Errorf("server saw handshake %+v, want %+v", peer.Handshake, wantHandshake)
	}
	if peer.Username != "Steve" || peer.ServerHash != "" {
		t.Errorf("server saw username %q and hash %q", peer.Username, peer.ServerHash)
	}
	want := conn.Profile{UUID: javatest.OfflineUUID("Steve"), Username: "Steve"}
	if client.Profile() != want {
		t.Errorf("Profile = %+v, want %+v", client.Profile(), want)
	}
	if client.RemoteAddr().String() != server.Addr() || client.LocalAddr() == nil {
		t.Errorf("RemoteAddr = %v, LocalAddr = %v", client.RemoteAddr(), client.LocalAddr())
	}
	exchange(t, client, peer)
}

// fixedSRV answers every SRV lookup with one record.
type fixedSRV struct{ record net.SRV }

func (r fixedSRV) LookupSRV(context.Context, string, string, string) (string, []*net.SRV, error) {
	return "", []*net.SRV{&r.record}, nil
}

func TestDialReportsSRVTargetInHandshake(t *testing.T) {
	server := startServer(t, javatest.Config{})
	var dialed string
	opts := conn.Options{
		Username: "Steve",
		Resolver: fixedSRV{record: net.SRV{Target: "mc.example.org.", Port: 25570}},
		Dialer: func(ctx context.Context, network, address string) (net.Conn, error) {
			dialed = address
			return (&net.Dialer{}).DialContext(ctx, network, server.Addr())
		},
	}
	ctx := testContext(t)
	client, err := conn.Dial(ctx, "example.org", opts)
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	peer, err := server.Accept(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if dialed != "mc.example.org.:25570" {
		t.Errorf("dialed %q, want the SRV target and port", dialed)
	}
	if peer.Handshake.ServerAddress != "mc.example.org." || peer.Handshake.ServerPort != 25570 {
		t.Errorf("handshake names %q port %d, want the SRV target as returned", peer.Handshake.ServerAddress, peer.Handshake.ServerPort)
	}
}

func TestEncryptedLogin(t *testing.T) {
	for _, serverID := range []string{"", "server-id"} {
		server := startServer(t, javatest.Config{OnlineMode: true, ServerID: serverID})
		var joined []string
		client, peer := join(t, server, conn.Options{
			Username: "Alex",
			JoinSession: func(_ context.Context, serverHash string) error {
				joined = append(joined, serverHash)
				return nil
			},
		})
		if len(joined) != 1 || joined[0] != peer.ServerHash || peer.ServerHash == "" {
			t.Fatalf("server id %q: JoinSession saw %q, server computed %q", serverID, joined, peer.ServerHash)
		}
		if want := conn.ServerHash(serverID, peer.SharedSecret, server.PublicKey()); joined[0] != want {
			t.Fatalf("server id %q: hash %q is not the hash of the id, secret and key (%q)", serverID, joined[0], want)
		}
		if len(peer.SharedSecret) != 16 {
			t.Fatalf("shared secret is %d bytes, want 16", len(peer.SharedSecret))
		}
		if client.Profile().Username != "Alex" {
			t.Fatalf("Profile = %+v", client.Profile())
		}
		exchange(t, client, peer)
		// A second round catches cipher state that only works for the first packets.
		exchange(t, client, peer)
	}
}

func TestEncryptionRequestWithoutJoinSession(t *testing.T) {
	server := startServer(t, javatest.Config{OnlineMode: true})
	_, err := conn.Dial(testContext(t), server.Addr(), conn.Options{Username: "Alex"})
	if !errors.Is(err, conn.ErrAuthenticationRequired) {
		t.Fatalf("err = %v, want ErrAuthenticationRequired", err)
	}
}

func TestJoinSessionFailureIsReported(t *testing.T) {
	server := startServer(t, javatest.Config{OnlineMode: true})
	rejected := errors.New("invalid session")
	_, err := conn.Dial(testContext(t), server.Addr(), conn.Options{
		Username:    "Alex",
		JoinSession: func(context.Context, string) error { return rejected },
	})
	var sessionErr *conn.SessionError
	if !errors.As(err, &sessionErr) || !errors.Is(err, rejected) {
		t.Fatalf("err = %v, want a SessionError wrapping the session failure", err)
	}
}

func TestLoginDisconnect(t *testing.T) {
	const reason = `{"text":"You are not white-listed on this server!"}`
	server := startServer(t, javatest.Config{RefuseLogin: reason})
	_, err := conn.Dial(testContext(t), server.Addr(), conn.Options{Username: "Steve"})
	var disconnect *conn.DisconnectError
	if !errors.As(err, &disconnect) || disconnect.Reason != reason {
		t.Fatalf("err = %v, want a DisconnectError carrying the reason", err)
	}
}

// silentListener accepts connections and never answers them. It reports each accepted
// connection on accepted.
func silentListener(t *testing.T) (address string, accepted <-chan struct{}) {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	signal := make(chan struct{}, 1)
	done := make(chan struct{})
	go func() {
		defer close(done)
		var held []net.Conn
		defer func() {
			for _, c := range held {
				_ = c.Close()
			}
		}()
		for {
			c, err := listener.Accept()
			if err != nil {
				return
			}
			held = append(held, c)
			signal <- struct{}{}
		}
	}()
	t.Cleanup(func() {
		_ = listener.Close()
		<-done
	})
	return listener.Addr().String(), signal
}

func TestDialStopsWhenContextIsCancelledDuringLogin(t *testing.T) {
	address, accepted := silentListener(t)
	ctx, cancel := context.WithCancel(testContext(t))
	defer cancel()
	result := make(chan error, 1)
	go func() {
		_, err := conn.Dial(ctx, address, conn.Options{Username: "Steve"})
		result <- err
	}()
	<-accepted
	cancel()
	if err := <-result; !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestDialStopsWhenContextIsCancelledDuringSessionJoin(t *testing.T) {
	server := startServer(t, javatest.Config{OnlineMode: true})
	ctx, cancel := context.WithCancel(testContext(t))
	defer cancel()
	_, err := conn.Dial(ctx, server.Addr(), conn.Options{
		Username: "Alex",
		JoinSession: func(ctx context.Context, _ string) error {
			cancel()
			<-ctx.Done()
			return ctx.Err()
		},
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestDialWithCancelledContext(t *testing.T) {
	server := startServer(t, javatest.Config{})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := conn.Dial(ctx, server.Addr(), conn.Options{Username: "Steve"}); !errors.Is(err, context.Canceled) {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
}

func TestDialReportsConnectFailure(t *testing.T) {
	refused := errors.New("connection refused")
	_, err := conn.Dial(testContext(t), "127.0.0.1:1", conn.Options{
		Username: "Steve",
		Dialer:   func(context.Context, string, string) (net.Conn, error) { return nil, refused },
	})
	if !errors.Is(err, refused) {
		t.Fatalf("err = %v, want the dialer's error", err)
	}
}

func TestUnknownPacketIsSurfaced(t *testing.T) {
	server := startServer(t, javatest.Config{})
	client, peer := join(t, server, conn.Options{Username: "Steve"})
	if err := peer.WriteFrame([]byte{0x7e, 0xca, 0xfe}); err != nil {
		t.Fatal(err)
	}
	got, err := client.ReadPacket()
	want := &protocol.Unknown{PacketID: 0x7e, PacketState: protocol.StatePlay, PacketDirection: protocol.Clientbound, Data: []byte{0xca, 0xfe}}
	if err != nil || !reflect.DeepEqual(got, want) {
		t.Fatalf("ReadPacket = %+v, err %v; want %+v", got, err, want)
	}
	// The connection stays usable after an unknown packet.
	exchange(t, client, peer)
}

func TestMalformedFramesAreRejected(t *testing.T) {
	cases := []struct {
		name string
		send func(peer *javatest.Client) error
		want error
	}{
		{"bytes after the packet", func(peer *javatest.Client) error {
			return peer.WriteFrame([]byte{0x00, 0, 0, 0, 1, 0xff})
		}, protocol.ErrTrailingData},
		{"packet cut short", func(peer *javatest.Client) error {
			return peer.WriteFrame([]byte{0x00, 0, 0})
		}, io.ErrUnexpectedEOF},
		{"string longer than its field allows", func(peer *javatest.Client) error {
			return peer.WriteFrame(append([]byte{0x38, 17}, make([]byte, 20)...))
		}, nil},
		{"frame length wider than 21 bits", func(peer *javatest.Client) error {
			return peer.WriteBytes([]byte{0x80, 0x80, 0x80, 0x01})
		}, conn.ErrFrameTooLarge},
		{"connection closed inside a frame", func(peer *javatest.Client) error {
			if err := peer.WriteBytes([]byte{0x05, 0x00, 0x01}); err != nil {
				return err
			}
			return peer.Close()
		}, io.ErrUnexpectedEOF},
	}
	for _, tc := range cases {
		server := startServer(t, javatest.Config{})
		client, peer := join(t, server, conn.Options{Username: "Steve"})
		if err := tc.send(peer); err != nil {
			t.Fatalf("%s: %v", tc.name, err)
		}
		p, err := client.ReadPacket()
		if err == nil || (tc.want != nil && !errors.Is(err, tc.want)) {
			t.Errorf("%s: ReadPacket = %+v, err %v; want error %v", tc.name, p, err, tc.want)
		}
	}
}

func TestWritePacketRejectsWhatCannotBeSent(t *testing.T) {
	server := startServer(t, javatest.Config{})
	client, _ := join(t, server, conn.Options{Username: "Steve"})
	if err := client.WritePacket(&protocol.KeepAlive{Key: 1}); err == nil {
		t.Error("a clientbound packet was queued")
	}
	if err := client.WritePacket(&protocol.LoginStart{Username: "Steve"}); err == nil {
		t.Error("a login packet was queued in the play state")
	}
	// The body of the largest unknown packet that still frames, then one byte more.
	fits := &protocol.Unknown{PacketID: 0x7e, PacketState: protocol.StatePlay, PacketDirection: protocol.Serverbound, Data: make([]byte, conn.MaxFrameSize-1)}
	if err := client.WritePacket(fits); err != nil {
		t.Errorf("frame of the largest size: %v", err)
	}
	fits.Data = append(fits.Data, 0)
	if err := client.WritePacket(fits); !errors.Is(err, conn.ErrFrameTooLarge) {
		t.Errorf("frame one byte too large: err = %v, want ErrFrameTooLarge", err)
	}
}

func TestCloseUnblocksReadPacket(t *testing.T) {
	server := startServer(t, javatest.Config{})
	client, _ := join(t, server, conn.Options{Username: "Steve", ReadTimeout: -1})
	result := make(chan error, 1)
	go func() {
		_, err := client.ReadPacket()
		result <- err
	}()
	if err := client.Close(); err != nil {
		t.Fatal(err)
	}
	if err := <-result; err == nil {
		t.Fatal("ReadPacket returned a packet from a closed connection")
	}
}

func TestServerCloseEndsReadPacket(t *testing.T) {
	server := startServer(t, javatest.Config{})
	client, peer := join(t, server, conn.Options{Username: "Steve"})
	if err := peer.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ReadPacket(); !errors.Is(err, io.EOF) {
		t.Fatalf("err = %v, want EOF", err)
	}
}
