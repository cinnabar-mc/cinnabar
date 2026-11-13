package translate

import (
	"bytes"
	"fmt"

	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// clientPackets constructs the Bedrock packets a client sends, by id.
var clientPackets = packet.NewClientPool()

// encodeBedrock returns a Bedrock packet's header and payload, the form the session relay
// carries.
func encodeBedrock(pk packet.Packet) []byte {
	var buf bytes.Buffer
	header := packet.Header{PacketID: pk.ID()}
	_ = header.Write(&buf)
	pk.Marshal(protocol.NewWriter(&buf, 0))
	return buf.Bytes()
}

// decodeBedrock decodes one encoded client packet. Packets the pool does not know return
// nil without an error: the translator has nothing to do with them.
func decodeBedrock(data []byte) (pk packet.Packet, err error) {
	buf := bytes.NewBuffer(data)
	var header packet.Header
	if err := header.Read(buf); err != nil {
		return nil, fmt.Errorf("read packet header: %w", err)
	}
	construct, ok := clientPackets[header.PacketID]
	if !ok {
		return nil, nil
	}
	pk = construct()
	defer func() {
		if recovered := recover(); recovered != nil {
			pk, err = nil, fmt.Errorf("decode packet %d: %v", header.PacketID, recovered)
		}
	}()
	pk.Marshal(protocol.NewReader(buf, 0, true))
	return pk, nil
}

// bedrockPacketID reads the packet id from an encoded packet's header.
func bedrockPacketID(data []byte) (uint32, bool) {
	var header packet.Header
	if err := header.Read(bytes.NewBuffer(data)); err != nil {
		return 0, false
	}
	return header.PacketID, true
}
