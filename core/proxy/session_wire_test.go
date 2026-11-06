package proxy

import (
	"bytes"
	"errors"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// A selected session packet that fails to decode stays raw and reports why.
func TestRawSessionPacketsMarkUndecodableSelectedPackets(t *testing.T) {
	text := encodeTestPacket(&packet.Text{TextType: packet.TextTypeRaw, Message: "hi"})
	malformed := append(bytes.Clone(text), 0xff)
	raws, err := rawSessionPackets([][]byte{malformed, text}, func(id uint32) bool { return id == packet.IDText },
		minecraft.DefaultProtocol.Packets(false), 0)
	if err != nil {
		t.Fatal(err)
	}
	if raws[0].Decoded != nil || !errors.Is(raws[0].DecodeErr, errUndecodableSessionPacket) || !bytes.Equal(raws[0].Data, malformed) {
		t.Fatalf("malformed packet = %+v, want raw with a decode error", raws[0])
	}
	if raws[1].Decoded == nil || raws[1].DecodeErr != nil {
		t.Fatalf("valid packet = %+v, want decoded", raws[1])
	}
}

// rawCaptureSink records whether forwarded packets were written raw or re-encoded.
type rawCaptureSink struct {
	*fakeUpstream
	raw       [][]byte
	reencoded int
}

func (s *rawCaptureSink) WritePacketRaw(data []byte) error {
	s.raw = append(s.raw, data)
	return nil
}

func (s *rawCaptureSink) WritePacket(packet.Packet) error {
	s.reencoded++
	return nil
}

// An inspected packet that failed to decode is forwarded exactly as received.
func TestForwardPacketPassesUndecodableInspectedPacketsUnchanged(t *testing.T) {
	sink := &rawCaptureSink{fakeUpstream: newFakeUpstream(nil)}
	data := []byte{9, 1, 2, 3}
	raw := minecraft.RawPacket{ID: packet.IDText, Data: data, DecodeErr: errUndecodableSessionPacket}
	if err := forwardPacket(sink, raw, login.IdentityData{DisplayName: "Steve", Identity: "id"}); err != nil {
		t.Fatal(err)
	}
	if len(sink.raw) != 1 || !bytes.Equal(sink.raw[0], data) || sink.reencoded != 0 {
		t.Fatalf("forwarded raw=%x reencoded=%d, want the received bytes once", sink.raw, sink.reencoded)
	}
}
