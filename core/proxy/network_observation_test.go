package proxy

import (
	"fmt"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
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

// The dialled upstream must keep forwarding the transport's connected-ping round trip, or
// latencySampler silently falls back to the acknowledgement estimate.
var _ interface{ AveragePing() (time.Duration, bool) } = (*minecraft.Conn)(nil)

// measuredUpstream reports fixed transport measurements; the zero value measures nothing.
type measuredUpstream struct {
	*fakeUpstream
	ping     time.Duration
	pinged   bool
	halfRTT  time.Duration
	estimate bool
}

// AveragePing returns the configured connected-ping round trip.
func (upstream measuredUpstream) AveragePing() (time.Duration, bool) {
	return upstream.ping, upstream.pinged
}

// Latency returns the configured half-RTT estimate, panicking like adapters without an estimator.
func (upstream measuredUpstream) Latency() time.Duration {
	if !upstream.estimate {
		panic("no latency estimator")
	}
	return upstream.halfRTT
}

func TestLatencySamplerPrefersConnectedPingRoundTrip(t *testing.T) {
	base := newFakeUpstream(nil)
	for _, test := range []struct {
		name     string
		upstream measuredUpstream
		want     *uint32
	}{
		// The acknowledgement estimate includes the server's ACK hold; pongs do not.
		{"ping", measuredUpstream{fakeUpstream: base, ping: 30600 * time.Microsecond, pinged: true, halfRTT: 47 * time.Millisecond, estimate: true}, new(uint32(31))},
		{"zero ping", measuredUpstream{fakeUpstream: base, pinged: true, halfRTT: 47 * time.Millisecond, estimate: true}, new(uint32(0))},
		{"no pong yet", measuredUpstream{fakeUpstream: base, halfRTT: 47 * time.Millisecond, estimate: true}, new(uint32(94))},
		{"unmeasured", measuredUpstream{fakeUpstream: base}, nil},
	} {
		got := latencySampler(test.upstream)()
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
