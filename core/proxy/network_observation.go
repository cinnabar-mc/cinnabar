package proxy

import "time"

// NetworkObservation identifies the active upstream and its current transport RTT.
// A nil sampler withdraws this attempt; unsupported or unmeasured RTT stays nil.
type NetworkObservation struct {
	AttemptID       uint64
	LocalRuntimeID  uint64
	RoundTripMillis func() *uint32
}

// latencySampler reports the upstream's round trip without measuring the local bridge. It prefers
// the transport's average connected-ping round trip, which excludes how long the server holds its
// acknowledgements, and otherwise doubles the transport's half-RTT estimate.
func latencySampler(upstream upstreamSession) func() *uint32 {
	pinger, pings := upstream.(interface{ AveragePing() (time.Duration, bool) })
	estimator, estimates := upstream.(interface{ Latency() time.Duration })
	if !pings && !estimates {
		return func() *uint32 { return nil }
	}
	return func() (sample *uint32) {
		// Some transport adapters expose Conn.Latency but have no underlying estimator.
		defer func() {
			if recover() != nil {
				sample = nil
			}
		}()
		if pings {
			if rtt, ok := pinger.AveragePing(); ok {
				return pingMillis(rtt)
			}
		}
		if estimates {
			return roundTripMillis(estimator.Latency())
		}
		return nil
	}
}

// pingMillis rounds a measured round trip down to whole milliseconds, as vanilla averages its ping
// history. A measured zero stays zero rather than becoming unknown.
func pingMillis(rtt time.Duration) *uint32 {
	const maxMillis = uint64(^uint32(0))
	value := uint32(min(maxMillis, uint64(max(rtt, 0)/time.Millisecond)))
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
