package proxy

import (
	"fmt"
	"net"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
)

func TestRoundTripMillisPreservesUnknownAndRoundsFullRTT(t *testing.T) {
	for _, missing := range []time.Duration{-time.Second, 0} {
		if roundTripMillis(missing) != nil {
			t.Fatal("unmeasured latency must remain unknown")
		}
	}
	for _, test := range []struct {
		half time.Duration
		want uint32
	}{
		{time.Nanosecond, 1}, {1250 * time.Microsecond, 3}, {25 * time.Millisecond, 50}, {time.Duration(1<<63 - 1), ^uint32(0)},
	} {
		if got := roundTripMillis(test.half); got == nil || *got != test.want {
			t.Fatalf("half RTT %v: got %v, want %d", test.half, got, test.want)
		}
	}
}

// addressedUpstream reports a fixed half-RTT estimate from a transport with the given address.
type addressedUpstream struct {
	*fakeUpstream
	half time.Duration
	addr net.Addr
}

// Latency returns the configured half-RTT estimate.
func (upstream addressedUpstream) Latency() time.Duration { return upstream.half }

// RemoteAddr returns the configured transport address.
func (upstream addressedUpstream) RemoteAddr() net.Addr { return upstream.addr }

func TestLatencySamplerReportsVanillaPingPerTransport(t *testing.T) {
	base := newFakeUpstream(nil)
	rakNet := &net.UDPAddr{IP: net.IPv4(198, 51, 100, 4), Port: 19132}
	for _, test := range []struct {
		name string
		half time.Duration
		addr net.Addr
		want *uint32
	}{
		// Vanilla halves the selected ICE pair's truncated millisecond round trip over NetherNet.
		{"nethernet", 17750 * time.Microsecond, &nethernet.Addr{}, new(uint32(17))},
		{"nethernet odd round trip", 24999 * time.Microsecond, &nethernet.Addr{}, new(uint32(24))},
		{"nethernet under a millisecond", 300 * time.Microsecond, &nethernet.Addr{}, new(uint32(0))},
		{"nethernet unmeasured", 0, &nethernet.Addr{}, nil},
		// RakNet keeps the full round trip vanilla reports there.
		{"raknet", 17750 * time.Microsecond, rakNet, new(uint32(36))},
	} {
		got := latencySampler(addressedUpstream{fakeUpstream: base, half: test.half, addr: test.addr})()
		if (got == nil) != (test.want == nil) || got != nil && *got != *test.want {
			t.Fatalf("%s: got %s, want %s", test.name, describeMillis(got), describeMillis(test.want))
		}
	}
}

// describeMillis formats an optional millisecond sample for failure messages.
func describeMillis(millis *uint32) string {
	if millis == nil {
		return "unknown"
	}
	return fmt.Sprintf("%dms", *millis)
}
