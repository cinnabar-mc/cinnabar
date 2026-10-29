package proxy

import (
	"context"
	"fmt"
	"strconv"
	"strings"

	"github.com/df-mc/go-nethernet"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/lanworlds"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// LANWorlds finds a listed NetherNet LAN world and the signaling that reaches it; release lets the
// signaling close once the session ends.
type LANWorlds interface {
	Find(ctx context.Context, networkID uint64) (world minecraft.LANWorld, signaling nethernet.Signaling, release func(), err error)
}

// releaseCloser adapts a release func to the target's closer.
type releaseCloser func()

func (r releaseCloser) Close() error {
	r()
	return nil
}

// lanTargetPrefix names a NetherNet LAN world by its network ID.
const lanTargetPrefix = bridgecontract.LANTargetPrefix

// isLANTarget reports whether address names a NetherNet LAN world.
func isLANTarget(address string) bool {
	return strings.HasPrefix(strings.ToLower(strings.TrimSpace(address)), lanTargetPrefix)
}

// resolveLANTarget joins a RakNet LAN world by address, or a listed NetherNet LAN world through
// the LAN browser's signaling. For NetherNet it logs
// in online when signed in and the host accepts online login, and self-signed otherwise when the
// host accepts that; either way it sends the host's advertised nonce.
func resolveLANTarget(ctx context.Context, address string, lan LANWorlds, account *authcache.Account) (*resolvedUpstreamTarget, error) {
	id, hostPort, ok := lanworlds.ParseTarget(strings.TrimSpace(address)[len(lanTargetPrefix):])
	if !ok {
		return nil, fmt.Errorf("invalid LAN world target %q", address)
	}
	if hostPort != "" {
		// A RakNet LAN world is joined directly, without the NetherNet probe of servers added by
		// address, and within the RakNet connection budget even when signed out.
		return &resolvedUpstreamTarget{address: hostPort, network: sessionRakNet{remoteRakNet()}}, nil
	}
	if lan == nil {
		return nil, fmt.Errorf("LAN worlds are unavailable")
	}
	findCtx, cancel := context.WithTimeout(ctx, localLANDiscoveryTimeout)
	defer cancel()
	world, signaling, release, err := lan.Find(findCtx, id)
	if err != nil {
		return nil, fmt.Errorf("LAN world: %w", err)
	}
	data := world.ServerData
	offline := account == nil || !data.AcceptsOnlineAuth
	if offline && !data.AcceptsSelfSignedAuth {
		release()
		if account == nil {
			return nil, fmt.Errorf("LAN world %q needs an Xbox sign-in", data.LevelName)
		}
		return nil, fmt.Errorf("LAN world %q accepts no login Cinnabar can offer", data.LevelName)
	}
	return &resolvedUpstreamTarget{
		address: strconv.FormatUint(id, 10), local: releaseCloser(release), offline: offline,
		hostOffline: offline && account != nil,
		// LAN hosts answer without an SDP identity.
		network:    minecraft.NetherNet{Signaling: signaling, Dialer: nethernet.Dialer{API: localNetherNetAPI(), AllowIdentitylessServer: true}},
		clientData: func(client *login.ClientData) { client.Nonce = data.Nonce },
	}, nil
}
