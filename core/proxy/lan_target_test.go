package proxy

import (
	"context"
	"errors"
	"testing"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/discovery"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"golang.org/x/oauth2"
)

// fakeLANWorlds lists one NetherNet world and counts releases.
type fakeLANWorlds struct {
	world    minecraft.LANWorld
	released int
}

func (f *fakeLANWorlds) Find(_ context.Context, id uint64) (minecraft.LANWorld, nethernet.Signaling, func(), error) {
	if id != f.world.NetworkID {
		return minecraft.LANWorld{}, nil, nil, errors.New("not listed")
	}
	return f.world, nil, func() { f.released++ }, nil
}

// A LAN world logs in online only when signed in and the host accepts it, else self-signed, and
// always sends the host's nonce.
func TestResolveLANTargetChoosesLoginAndNonce(t *testing.T) {
	account := authcache.NewAccount(t.Context(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil)
	for _, tt := range []struct {
		name        string
		online      bool
		selfSigned  bool
		account     *authcache.Account
		wantOffline bool
		wantErr     bool
	}{
		{name: "signed in, online accepted", online: true, selfSigned: true, account: account},
		{name: "signed out", online: true, selfSigned: true, wantOffline: true},
		{name: "signed in, self-signed only", selfSigned: true, account: account, wantOffline: true},
		{name: "signed out, online only", online: true, wantErr: true},
	} {
		t.Run(tt.name, func(t *testing.T) {
			lan := &fakeLANWorlds{world: minecraft.LANWorld{NetherNet: true, NetworkID: 42, ServerData: discovery.ServerData{
				LevelName: "World", Nonce: "nonce-1", AcceptsOnlineAuth: tt.online, AcceptsSelfSignedAuth: tt.selfSigned,
			}}}
			target, err := resolveLANTarget(t.Context(), "lan/42", lan, tt.account)
			if tt.wantErr {
				if err == nil || lan.released != 1 {
					t.Fatalf("resolveLANTarget() = %v with %d releases, want an error and a release", err, lan.released)
				}
				return
			}
			if err != nil {
				t.Fatalf("resolveLANTarget() error = %v", err)
			}
			if target.offline != tt.wantOffline || target.address != "42" {
				t.Fatalf("target offline = %t address %q, want offline %t", target.offline, target.address, tt.wantOffline)
			}
			var data login.ClientData
			target.clientData(&data)
			if data.Nonce != "nonce-1" {
				t.Fatalf("nonce = %q, want the host's", data.Nonce)
			}
			if err := target.close(); err != nil || lan.released != 1 {
				t.Fatalf("close() = %v with %d releases, want one", err, lan.released)
			}
		})
	}
}

// Malformed or unlisted LAN targets fail before dialing.
func TestResolveLANTargetRejectsBadTargets(t *testing.T) {
	lan := &fakeLANWorlds{world: minecraft.LANWorld{NetherNet: true, NetworkID: 42}}
	for _, address := range []string{"lan/", "lan/0", "lan/abc", "lan/43", "lan/raknet/host"} {
		if _, err := resolveLANTarget(t.Context(), address, lan, nil); err == nil {
			t.Fatalf("resolveLANTarget(%q) succeeded", address)
		}
	}
	if _, err := resolveLANTarget(t.Context(), "lan/42", nil, nil); err == nil {
		t.Fatal("resolveLANTarget without a LAN service succeeded")
	}
	if !isLANTarget(" LAN/42") || isLANTarget("lanparty.example:19132") {
		t.Fatal("isLANTarget misclassified an address")
	}
}

// A RakNet LAN world joins its address over plain RakNet, without the LAN browser.
func TestResolveLANTargetJoinsRakNetDirectly(t *testing.T) {
	target, err := resolveLANTarget(t.Context(), "lan/raknet/192.168.1.4:19132", nil, nil)
	if err != nil {
		t.Fatalf("resolveLANTarget() error = %v", err)
	}
	if _, ok := target.network.(sessionRakNet); !ok || !minecraft.IsRakNet(target.network) || target.address != "192.168.1.4:19132" {
		t.Fatalf("target = %T %q, want budgeted RakNet at the world's address", target.network, target.address)
	}
}

// A signed-in player logs into a self-signed-only LAN host offline, but a transfer from it logs in online.
func TestLANHostOfflineLoginStaysOnTheHost(t *testing.T) {
	account := authcache.NewAccount(t.Context(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil)
	lan := &fakeLANWorlds{world: minecraft.LANWorld{NetherNet: true, NetworkID: 42, ServerData: discovery.ServerData{AcceptsSelfSignedAuth: true}}}
	target, err := resolveLANTarget(t.Context(), "lan/42", lan, account)
	if err != nil {
		t.Fatalf("resolveLANTarget() error = %v", err)
	}
	if !target.offline {
		t.Fatal("a self-signed-only host got an online login")
	}
	tokens := oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "account"})
	if hop := localHopDialer(target, target.address, minecraft.Dialer{}, tokens); hop.TokenSource != nil {
		t.Fatal("the LAN host itself got an online login")
	}
	if hop := localHopDialer(target, "play.example.net:19132", minecraft.Dialer{}, tokens); hop.TokenSource == nil {
		t.Fatal("a transfer from the LAN host kept the offline login")
	}
}
