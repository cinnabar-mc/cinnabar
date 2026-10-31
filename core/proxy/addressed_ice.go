package proxy

import (
	"context"
	"crypto/ecdsa"
	"net"
	"strings"

	"github.com/df-mc/go-nethernet"
	"github.com/sandertv/gophertunnel/minecraft"
)

const addressedSTUNServer = "stun:stun.cloudflare.com:3478"

// Select keeps loopback joins independent of external address discovery.
func (n addressedServerNetwork) Select(ctx context.Context, address string) (minecraft.Network, error) {
	host, _, err := net.SplitHostPort(address)
	if err != nil {
		host = address
	}
	if strings.EqualFold(host, "localhost") || net.ParseIP(host).IsLoopback() {
		n.NetherNet.Dialer.Credentials = nil
	}
	return n.AddressNetwork.Select(ctx, address)
}

// DialContextIdentityProvider preserves per-address gathering for signed-in joins.
func (n addressedServerNetwork) DialContextIdentityProvider(ctx context.Context, address, token string, key *ecdsa.PrivateKey, domain string) (net.Conn, error) {
	selected, err := n.Select(ctx, address)
	if err != nil {
		return nil, err
	}
	if dialer, ok := selected.(identityProviderDialer); ok {
		return dialer.DialContextIdentityProvider(ctx, address, token, key, domain)
	}
	return dialTransport(ctx, selected, address)
}

// addressedICECredentials supplies public address discovery for HTTP peers whose
// signaling endpoint has no ICE credential provider. Each join owns its values.
func addressedICECredentials(ctx context.Context) (*nethernet.Credentials, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return &nethernet.Credentials{ICEServers: []nethernet.ICEServer{
		{URLs: []string{addressedSTUNServer}},
	}}, nil
}
