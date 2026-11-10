package proxy

import (
	"net"
	"time"

	"github.com/df-mc/go-nethernet"
)

// NetworkObservation identifies the active upstream and the ping vanilla's network status reports
// for it. A nil sampler withdraws this attempt; an unsupported or unmeasured ping stays nil.
type NetworkObservation struct {
	AttemptID       uint64
	LocalRuntimeID  uint64
	RoundTripMillis func() *uint32
}

// latencySampler reports the upstream's ping as vanilla's network status does, without measuring
// the local bridge: the full round trip over RakNet, and half the selected ICE candidate pair's
// round trip over NetherNet.
func latencySampler(upstream upstreamSession) func() *uint32 {
	connection, ok := upstream.(interface{ Latency() time.Duration })
	if !ok {
		return func() *uint32 { return nil }
	}
	convert := roundTripMillis
	if isNetherNet(upstream) {
		convert = netherNetPingMillis
	}
	return func() (sample *uint32) {
		// Some transport adapters expose Conn.Latency but have no underlying estimator.
		defer func() {
			if recover() != nil {
				sample = nil
			}
		}()
		return convert(connection.Latency())
	}
}

// isNetherNet reports whether upstream runs over a NetherNet (WebRTC) transport.
func isNetherNet(upstream upstreamSession) bool {
	addressed, ok := upstream.(interface{ RemoteAddr() net.Addr })
	if !ok {
		return false
	}
	_, ok = addressed.RemoteAddr().(*nethernet.Addr)
	return ok
}

// netherNetPingMillis converts go-nethernet's half ICE round trip into vanilla's NetherNet ping.
// Vanilla truncates the selected pair's round trip to whole milliseconds with float32 arithmetic,
// then halves it with integer division. A measured zero stays zero rather than becoming unknown.
func netherNetPingMillis(half time.Duration) *uint32 {
	if half <= 0 {
		return nil
	}
	roundTripMillis := int64(float32(half.Seconds()*2) * 1000)
	value := uint32(min(int64(^uint32(0)), roundTripMillis/2))
	return &value
}

// roundTripMillis rounds a positive half-RTT estimate up to full milliseconds.
func roundTripMillis(half time.Duration) *uint32 {
	if half <= 0 {
		return nil
	}
	const maxMillis = uint64(^uint32(0))
	// Quotient/remainder arithmetic avoids overflowing the duration when doubling.
	whole := uint64(half / time.Millisecond)
	fraction := half % time.Millisecond
	millis := min(maxMillis, whole*2+uint64((fraction*2+time.Millisecond-1)/time.Millisecond))
	value := uint32(millis)
	return &value
}
