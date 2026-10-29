package lanworlds

import (
	"context"
	"log/slog"
	"net"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
)

// A fake RakNet and NetherNet responder each appear once while answering, and drop out after
// they stop: refreshed answers never duplicate a world, and a world on both transports is
// listed once per transport, NetherNet first.
func TestFakeLANRespondersAppearDedupeAndExpire(t *testing.T) {
	rakNet, err := raknet.Listen("127.0.0.1:0")
	if err != nil {
		t.Fatalf("raknet.Listen: %v", err)
	}
	rakNet.PongData([]byte("MCPE;Host;924;1.26.60;1;8;42;LAN World;Creative;1;0;0;0;"))
	netherNetAddr := freeUDPAddr(t)
	netherNet, err := discovery.ListenConfig{}.Listen(netherNetAddr.String())
	if err != nil {
		t.Fatalf("discovery.Listen: %v", err)
	}
	netherNet.ServerData(&discovery.ServerData{ServerName: "Host", LevelName: "LAN World", Version: "1.26.60",
		GameType: 1, PlayerCount: 1, MaxPlayerCount: 8, AcceptsSelfSignedAuth: true, ConnectionType: 4})
	service := newService(func() (Browser, error) {
		return minecraft.LANBrowserConfig{
			RakNetPort:       rakNet.Addr().(*net.UDPAddr).Port,
			NetherNetAddress: netherNetAddr,
			Log:              slog.New(slog.DiscardHandler),
		}.Browse()
	}, slog.New(slog.DiscardHandler))
	defer service.Close()

	worlds := func() []bridgecontract.LANWorld {
		list, err := service.Worlds()
		if err != nil {
			t.Fatalf("Worlds: %v", err)
		}
		return list
	}
	testwait.Eventually(t, testwait.DefaultTimeout, "both responders listed", func() bool { return len(worlds()) == 2 })
	// Several more ping rounds and discovery broadcasts pass without adding rows.
	if testwait.WaitUntil(3*time.Second, func() bool { return len(worlds()) != 2 }) {
		t.Fatalf("worlds = %+v, want exactly one row per responder", worlds())
	}
	got := worlds()
	if got[0].Transport != TransportNetherNet || got[1].Transport != TransportRakNet {
		t.Fatalf("worlds = %+v, want NetherNet then RakNet", got)
	}
	for _, world := range got {
		if world.LevelName != "LAN World" || world.ServerName != "Host" || world.GameMode != "Creative" || world.MaxPlayers != 8 {
			t.Fatalf("world = %+v", world)
		}
	}
	if got[1].Address != RakNetTarget(rakNet.Addr().String()) {
		t.Fatalf("RakNet address = %q, want %q", got[1].Address, rakNet.Addr())
	}

	_ = netherNet.Close()
	_ = rakNet.Close()
	testwait.Eventually(t, 15*time.Second, "silent responders to expire", func() bool { return len(worlds()) == 0 })
}

// The browser opens on the first listing, stays open while a join holds it, and closes once the
// launcher has stopped listing for the idle timeout.
func TestBrowserClosesWhenIdleAndUnheld(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var opened, closed atomic.Int32
		browser := &fakeBrowser{worlds: []minecraft.LANWorld{{NetherNet: true, NetworkID: 7, ServerName: "h", LevelName: "w"}}, closed: &closed}
		service := newService(func() (Browser, error) {
			opened.Add(1)
			return browser, nil
		}, slog.New(slog.DiscardHandler))
		defer service.Close()

		if _, err := service.Worlds(); err != nil || opened.Load() != 1 {
			t.Fatalf("Worlds() = %v after %d opens", err, opened.Load())
		}
		_, _, release, err := service.Find(t.Context(), 7)
		if err != nil {
			t.Fatalf("Find: %v", err)
		}
		time.Sleep(2 * idleTimeout)
		synctest.Wait()
		if closed.Load() != 0 {
			t.Fatal("closed the browser a join still holds")
		}
		release()
		time.Sleep(idleTimeout + time.Second)
		synctest.Wait()
		if closed.Load() != 1 {
			t.Fatalf("closes = %d after the idle timeout, want 1", closed.Load())
		}
		if _, err := service.Worlds(); err != nil || opened.Load() != 2 {
			t.Fatalf("Worlds() = %v after %d opens, want a reopened browser", err, opened.Load())
		}
	})
}

// A join waits for its world to answer and fails once the context ends without it.
func TestFindFailsForAWorldThatNeverAnswers(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		service := newService(func() (Browser, error) { return &fakeBrowser{closed: new(atomic.Int32)}, nil }, slog.New(slog.DiscardHandler))
		defer service.Close()
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		if _, _, _, err := service.Find(ctx, 9); err == nil {
			t.Fatal("Find returned a world that never answered")
		}
		if service.leases != 0 {
			t.Fatalf("leases = %d after a failed Find", service.leases)
		}
	})
}

// A NetherNet world joins by its network ID and a RakNet world by host:port.
func TestWireWorldAddresses(t *testing.T) {
	netherNet := wireWorld(minecraft.LANWorld{NetherNet: true, NetworkID: 12345})
	if netherNet.Address != bridgecontract.LANTargetPrefix+"12345" || netherNet.Transport != TransportNetherNet {
		t.Fatalf("NetherNet row = %+v", netherNet)
	}
	rakNet := wireWorld(minecraft.LANWorld{Address: &net.UDPAddr{IP: net.IPv4(192, 168, 1, 4), Port: 19132}, PlayerCount: -1})
	if rakNet.Address != "lan/raknet/192.168.1.4:19132" || rakNet.Transport != TransportRakNet || rakNet.Players != 0 {
		t.Fatalf("RakNet row = %+v", rakNet)
	}
}

// fakeBrowser lists fixed worlds and counts closes.
type fakeBrowser struct {
	worlds []minecraft.LANWorld
	closed *atomic.Int32
}

func (b *fakeBrowser) Worlds() []minecraft.LANWorld   { return b.worlds }
func (b *fakeBrowser) NetherNet() *discovery.Listener { return nil }
func (b *fakeBrowser) Close() error {
	b.closed.Add(1)
	return nil
}

// freeUDPAddr returns a loopback UDP address that was free a moment ago.
func freeUDPAddr(t *testing.T) *net.UDPAddr {
	t.Helper()
	conn, err := net.ListenPacket("udp4", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("ListenPacket: %v", err)
	}
	defer conn.Close()
	return conn.LocalAddr().(*net.UDPAddr)
}

// LAN target values name a NetherNet network ID or a RakNet host:port.
func TestParseTarget(t *testing.T) {
	if id, hostPort, ok := ParseTarget("12345"); !ok || id != 12345 || hostPort != "" {
		t.Fatalf("ParseTarget(id) = %d, %q, %t", id, hostPort, ok)
	}
	if id, hostPort, ok := ParseTarget("raknet/192.168.1.4:19132"); !ok || id != 0 || hostPort != "192.168.1.4:19132" {
		t.Fatalf("ParseTarget(raknet) = %d, %q, %t", id, hostPort, ok)
	}
	for _, bad := range []string{"", "0", "abc", "raknet/", "raknet/host", "raknet/host:0", "raknet/:19132"} {
		if _, _, ok := ParseTarget(bad); ok {
			t.Fatalf("ParseTarget(%q) accepted", bad)
		}
	}
}
