package proxy

import (
	"bytes"
	"net"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// windowStep is one relayed packet: server-sent values are checked client-bound and client closes server-bound.
type windowStep struct {
	sent, want packet.Packet
}

func serverOpen(window, windowType, want byte) windowStep {
	return windowStep{
		&packet.ContainerOpen{WindowID: window, ContainerType: windowType, ContainerPosition: protocol.BlockPos{1, 2, 3}, ContainerEntityUniqueID: -1},
		&packet.ContainerOpen{WindowID: window, ContainerType: want, ContainerPosition: protocol.BlockPos{1, 2, 3}, ContainerEntityUniqueID: -1},
	}
}

func serverClose(window, windowType, want byte, serverSide bool) windowStep {
	return windowStep{
		&packet.ContainerClose{WindowID: window, ContainerType: windowType, ServerSide: serverSide},
		&packet.ContainerClose{WindowID: window, ContainerType: want, ServerSide: serverSide},
	}
}

// clientClose is a close the client sends and the relay reads after every earlier server step.
func clientClose(window, windowType, want byte) windowStep {
	step := serverClose(window, windowType, want, false)
	step.sent = clientPacket{step.sent.(*packet.ContainerClose)}
	return step
}

type clientPacket struct{ *packet.ContainerClose }

// Each interleaving keeps virtual menus storage-typed for the client and virtual-typed for the server,
// whatever order replacements, client closes, acknowledgements and server closes are relayed in.
func TestVirtualContainerWindowInterleavings(t *testing.T) {
	const (
		window    = 9
		virtual   = virtualWindowType
		storage   = storageWindowType
		inventory = protocol.WindowIDInventory
	)
	for _, test := range []struct {
		name  string
		steps []windowStep
	}{
		{name: "client close then virtual acknowledgement", steps: []windowStep{
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverClose(window, virtual, storage, false),
		}},
		{name: "client close then storage acknowledgement", steps: []windowStep{
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverClose(window, storage, storage, false),
		}},
		{name: "server close", steps: []windowStep{
			serverOpen(window, virtual, storage),
			serverClose(window, virtual, storage, true),
		}},
		{name: "storage menu unchanged", steps: []windowStep{
			serverOpen(window, storage, storage),
			clientClose(window, storage, storage),
			serverClose(window, storage, storage, false),
		}},
		{name: "personal inventory unchanged", steps: []windowStep{
			serverOpen(inventory, virtual, virtual),
			clientClose(inventory, virtual, virtual),
			serverClose(inventory, virtual, virtual, false),
			clientClose(inventory, storage, storage),
		}},
		{name: "virtual reopen before acknowledgement then server close", steps: []windowStep{
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverOpen(window, virtual, storage),
			serverClose(window, virtual, storage, false),
			serverClose(window, virtual, storage, true),
		}},
		{name: "virtual reopen before server close then acknowledgement", steps: []windowStep{
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverOpen(window, virtual, storage),
			serverClose(window, virtual, storage, true),
			serverClose(window, virtual, storage, false),
		}},
		{name: "virtual replacement relayed before an older client close", steps: []windowStep{
			serverOpen(window, virtual, storage),
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverClose(window, virtual, storage, true),
			serverClose(window, virtual, storage, false),
		}},
		{name: "storage replacement relayed before an older virtual close", steps: []windowStep{
			serverOpen(window, virtual, storage),
			serverOpen(window, storage, storage),
			clientClose(window, storage, storage),
			serverClose(window, storage, storage, true),
			serverClose(window, storage, storage, false),
		}},
		{name: "storage menu replaced by virtual menu without a close", steps: []windowStep{
			serverOpen(window, storage, storage),
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverClose(window, virtual, storage, false),
		}},
		{name: "repeated virtual opens then one close", steps: []windowStep{
			serverOpen(window, virtual, storage),
			serverOpen(window, virtual, storage),
			serverOpen(window, virtual, storage),
			clientClose(window, storage, virtual),
			serverClose(window, virtual, storage, false),
			clientClose(window, storage, virtual),
			serverClose(window, virtual, storage, false),
		}},
		{name: "other window types keep their close", steps: []windowStep{
			serverOpen(window, 2, 2),
			serverOpen(window, virtual, storage),
			clientClose(window, 2, 2),
			serverClose(window, 2, 2, false),
		}},
		{name: "window IDs translate independently", steps: []windowStep{
			serverOpen(window, virtual, storage),
			serverOpen(window+1, storage, storage),
			clientClose(window+1, storage, storage),
			clientClose(window, storage, virtual),
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			local, peer := net.Pipe()
			session := newSessionConn(streamnet.NewFramedConn(local))
			client := streamnet.NewFramedConn(peer)
			t.Cleanup(func() {
				_ = session.Close()
				_ = client.Close()
			})
			// Client-bound packets stay in the forming batch, as an unflushed replacement would.
			for index, step := range test.steps {
				var got []byte
				if sent, ok := step.sent.(clientPacket); ok {
					got = relayClientPacket(t, session, client, sent.ContainerClose)
				} else {
					got = relayServerPacket(t, session, step.sent)
				}
				if want := encodeTestPacket(step.want); !bytes.Equal(got, want) {
					t.Fatalf("step %d relayed %+v as %x, want %+v", index, step.sent, got, step.want)
				}
			}
		})
	}
}

// relayServerPacket returns the client-bound bytes the session forms for value.
func relayServerPacket(t *testing.T, session *sessionConn, value packet.Packet) []byte {
	t.Helper()
	if err := session.WritePacket(value); err != nil {
		t.Fatal(err)
	}
	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatal(err)
	}
	return packets[len(packets)-1]
}

// relayClientPacket returns the server-bound bytes the session reads for a client close.
func relayClientPacket(t *testing.T, session *sessionConn, client *streamnet.FramedConn, value *packet.ContainerClose) []byte {
	t.Helper()
	frame := appendSessionBatchPacket([]byte{bridgecontract.SessionKindBatch}, encodeTestPacket(value))
	if _, err := client.Write(frame); err != nil {
		t.Fatal(err)
	}
	packets, err := session.ReadBatchRaw(nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(packets) != 1 {
		t.Fatalf("read %d packets, want one", len(packets))
	}
	return packets[0].Data
}

// A translated packet keeps header bits outside the packet ID.
func TestVirtualContainerKeepsPacketHeader(t *testing.T) {
	session := &sessionConn{serverPool: minecraft.DefaultProtocol.Packets(false)}
	data := encodeTestPacket(&packet.ContainerClose{WindowID: 9, ContainerType: virtualWindowType})
	header := new(bytes.Buffer)
	_ = (&packet.Header{PacketID: packet.IDContainerClose, SenderSubClient: 1, TargetSubClient: 2}).Write(header)
	data = append(header.Bytes(), data[1:]...)
	got := session.translateServerWindow(data)
	want := append(bytes.Clone(data[:len(data)-2]), storageWindowType, 0)
	if !bytes.Equal(got, want) {
		t.Fatalf("translated %x as %x, want %x", data, got, want)
	}
}
