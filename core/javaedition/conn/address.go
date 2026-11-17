package conn

import (
	"context"
	"fmt"
	"math"
	"net"
	"net/netip"
	"strconv"
	"strings"
)

// DefaultPort is the port a server address without one refers to.
const DefaultPort = 25565

// srvService and srvProto name the DNS SRV record 1.7.10 looks up: _minecraft._tcp.<host>.
const (
	srvService = "minecraft"
	srvProto   = "tcp"
)

// Resolver looks up DNS SRV records. *net.Resolver implements it.
type Resolver interface {
	LookupSRV(ctx context.Context, service, proto, name string) (cname string, addrs []*net.SRV, err error)
}

// Address is where a server is reached. Host and Port are also what the client reports to
// the server in its Handshake.
type Address struct {
	Host string
	Port uint16
}

// String returns the address in host:port form.
func (a Address) String() string { return net.JoinHostPort(a.Host, strconv.Itoa(int(a.Port))) }

// SplitAddress splits a server address as typed into the client into host and port. It
// follows 1.7.10: "host", "host:port" and "[ipv6]:port" are understood, an unreadable port
// falls back to DefaultPort, and text with more than one colon outside brackets is taken
// whole as the host. The port is not range-checked.
func SplitAddress(address string) (host string, port int, err error) {
	parts := splitDroppingTrailingEmpty(address, ":")
	if strings.HasPrefix(address, "[") {
		if end := strings.Index(address, "]"); end > 0 {
			host := address[1:end]
			rest := trimJava(address[end+1:])
			if strings.HasPrefix(rest, ":") {
				parts = []string{host, rest[1:]}
			} else {
				parts = []string{host}
			}
		}
	}
	if len(parts) > 2 {
		parts = []string{address}
	}
	if len(parts) == 0 {
		return "", 0, fmt.Errorf("server address %q names no host", address)
	}
	port = DefaultPort
	if len(parts) > 1 {
		port = parsePort(parts[1])
	}
	return parts[0], port, nil
}

// ResolveAddress turns a server address as typed into the client into the host and port to
// connect to. Like 1.7.10 it consults the _minecraft._tcp SRV record whenever the port is
// DefaultPort, written out or not, and uses the target, exactly as returned, and port of
// the first record the resolver reports. A failed lookup keeps the typed host. IP literals
// and an empty host skip the lookup: no record can exist for them.
func ResolveAddress(ctx context.Context, resolver Resolver, address string) (Address, error) {
	host, port, err := SplitAddress(address)
	if err != nil {
		return Address{}, err
	}
	if port == DefaultPort && resolver != nil && host != "" && !isIPLiteral(host) {
		if _, records, err := resolver.LookupSRV(ctx, srvService, srvProto, host); err == nil && len(records) > 0 {
			host, port = records[0].Target, int(records[0].Port)
		} else if ctxErr := ctx.Err(); ctxErr != nil {
			return Address{}, fmt.Errorf("resolve server address %q: %w", address, ctxErr)
		}
	}
	if port < 0 || port > math.MaxUint16 {
		return Address{}, fmt.Errorf("server address %q: port %d out of range", address, port)
	}
	return Address{Host: host, Port: uint16(port)}, nil
}

// splitDroppingTrailingEmpty splits s around sep and removes trailing empty pieces, which is
// how Java's String.split behaves.
func splitDroppingTrailingEmpty(s, sep string) []string {
	parts := strings.Split(s, sep)
	for len(parts) > 0 && parts[len(parts)-1] == "" {
		parts = parts[:len(parts)-1]
	}
	if len(parts) == 0 && s == "" {
		return []string{""}
	}
	return parts
}

// trimJava removes leading and trailing characters up to U+0020, like Java's String.trim.
func trimJava(s string) string {
	return strings.TrimFunc(s, func(r rune) bool { return r <= ' ' })
}

// parsePort reads a decimal port, returning DefaultPort when the text is not a 32-bit integer.
func parsePort(s string) int {
	port, err := strconv.ParseInt(trimJava(s), 10, 32)
	if err != nil {
		return DefaultPort
	}
	return int(port)
}

// isIPLiteral reports whether host is an IPv4 or IPv6 address rather than a name.
func isIPLiteral(host string) bool {
	_, err := netip.ParseAddr(host)
	return err == nil
}
