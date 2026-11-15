package translate

import (
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// probeTimestampScale is what the Bedrock client multiplies a latency probe's timestamp by
// in its reply.
const probeTimestampScale = 1_000_000

// probe is a server packet the 1.7.10 client answers once it has processed everything before
// it: a rejected transaction, which it confirms, or a keep-alive, which it echoes.
type probe struct {
	keepAlive bool
	key       int32
	window    int8
	action    int16
}

// probeClient forwards a probe to the Bedrock client as a latency probe, which the client
// answers only after applying every packet before it and sending the movement that came
// first. Anticheats time velocity, teleports and abilities by these answers, so they must
// come from the client in that order, not from the core when the packet arrives.
func (s *Session) probeClient(p probe) {
	st := &s.state
	st.nextProbe++
	st.probes[st.nextProbe] = p
	s.queue(&packet.NetworkStackLatency{Timestamp: st.nextProbe, NeedsResponse: true})
}

// answerProbe sends the 1.7.10 answer to the probe a latency reply echoes, if it is one.
func (s *Session) answerProbe(reply *packet.NetworkStackLatency) error {
	st := &s.state
	if reply.Timestamp%probeTimestampScale != 0 {
		return nil
	}
	id := reply.Timestamp / probeTimestampScale
	p, ok := st.probes[id]
	if !ok {
		return nil
	}
	delete(st.probes, id)
	if p.keepAlive {
		return s.send(&java.ClientKeepAlive{Key: p.key})
	}
	return s.send(&java.ClientConfirmTransaction{WindowID: p.window, Action: p.action, Accepted: true})
}
