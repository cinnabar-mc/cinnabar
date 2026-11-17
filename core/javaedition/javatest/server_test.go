package javatest_test

import (
	"context"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/conn"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
)

func TestOfflineUUID(t *testing.T) {
	// MD5 of "OfflinePlayer:Notch" with the version and variant bits of a name-based UUID.
	if got, want := javatest.OfflineUUID("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f"; got != want {
		t.Fatalf("OfflineUUID = %s, want %s", got, want)
	}
}

func TestServerAcceptsSeveralConnections(t *testing.T) {
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	ctx, cancel := context.WithTimeout(context.Background(), testwait.DefaultTimeout)
	defer cancel()
	for _, name := range []string{"Steve", "Alex"} {
		client, err := conn.Dial(ctx, server.Addr(), conn.Options{Username: name})
		if err != nil {
			t.Fatal(err)
		}
		defer client.Close()
		peer, err := server.Accept(ctx)
		if err != nil {
			t.Fatal(err)
		}
		if peer.Username != name || peer.UUID != client.Profile().UUID {
			t.Fatalf("accepted %q (%s), want %q (%s)", peer.Username, peer.UUID, name, client.Profile().UUID)
		}
	}
}

func TestAcceptReportsBrokenLogin(t *testing.T) {
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	raw, err := net.Dial("tcp", server.Addr())
	if err != nil {
		t.Fatal(err)
	}
	defer raw.Close()
	// A status request where the handshake belongs.
	if _, err := raw.Write([]byte{0x01, 0x01}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), testwait.DefaultTimeout)
	defer cancel()
	if client, err := server.Accept(ctx); err == nil {
		t.Fatalf("Accept returned %+v for a connection that sent no handshake", client)
	}
}

func TestAcceptEndsWithContextAndClose(t *testing.T) {
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := server.Accept(ctx); err != context.Canceled {
		t.Fatalf("err = %v, want context.Canceled", err)
	}
	server.Close()
	if _, err := server.Accept(context.Background()); err != net.ErrClosed {
		t.Fatalf("err = %v, want net.ErrClosed", err)
	}
}

func TestClientWriteBytesNeedsPlainConnection(t *testing.T) {
	server, err := javatest.Start(javatest.Config{OnlineMode: true})
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	ctx, cancel := context.WithTimeout(context.Background(), testwait.DefaultTimeout)
	defer cancel()
	client, err := conn.Dial(ctx, server.Addr(), conn.Options{
		Username:    "Alex",
		JoinSession: func(context.Context, string) error { return nil },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	peer, err := server.Accept(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := peer.WriteBytes([]byte{0x00}); err == nil {
		t.Fatal("raw bytes were written to an encrypted connection")
	}
	if err := peer.Write(&protocol.KeepAlive{Key: 9}); err != nil {
		t.Fatal(err)
	}
	if p, err := client.ReadPacket(); err != nil || p.(*protocol.KeepAlive).Key != 9 {
		t.Fatalf("ReadPacket = %+v, err %v", p, err)
	}
}
