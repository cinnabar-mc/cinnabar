package proxy

import (
	"bytes"
	"fmt"
	"log/slog"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSessionPreservesPersonalInventoryOpen(t *testing.T) {
	session := containerTestSession()
	open := encodeTestPacket(&packet.ContainerOpen{
		WindowID:                protocol.WindowIDInventory,
		ContainerType:           255,
		ContainerEntityUniqueID: 42,
	})
	if err := session.WritePacketRaw(open); err != nil {
		t.Fatal(err)
	}
	if got := takeContainerTestPacket(t, session); !bytes.Equal(got, open) {
		t.Fatalf("personal inventory open changed: %x, want %x", got, open)
	}
	close := encodeTestPacket(&packet.ContainerClose{WindowID: protocol.WindowIDInventory, ContainerType: 255})
	if err := session.WritePacketRaw(close); err != nil {
		t.Fatal(err)
	}
	if got := takeContainerTestPacket(t, session); !bytes.Equal(got, close) {
		t.Fatalf("personal inventory close changed: %x, want %x", got, close)
	}
}

func TestVirtualContainerTranslation(t *testing.T) {
	for _, serverSide := range []bool{false, true} {
		for _, closeType := range []byte{255, 0} {
			t.Run(fmt.Sprintf("server_side=%t/type=%d", serverSide, closeType), func(t *testing.T) {
				session := containerTestSession()
				open := &packet.ContainerOpen{
					WindowID:                9,
					ContainerType:           255,
					ContainerPosition:       protocol.BlockPos{1, 2, 3},
					ContainerEntityUniqueID: 51003,
				}
				if err := session.WritePacket(open); err != nil {
					t.Fatal(err)
				}
				open.ContainerType = 0
				if got, want := takeContainerTestPacket(t, session), encodeTestPacket(open); !bytes.Equal(got, want) {
					t.Fatalf("translated open = %x, want %x", got, want)
				}

				if !serverSide {
					close := &packet.ContainerClose{WindowID: open.WindowID, ContainerType: 0}
					got := readContainerTestClose(t, session, close)
					close.ContainerType = 255
					if want := encodeTestPacket(close); !bytes.Equal(got, want) {
						t.Fatalf("translated client close = %x, want %x", got, want)
					}
				}

				close := &packet.ContainerClose{WindowID: open.WindowID, ContainerType: closeType, ServerSide: serverSide}
				if err := session.WritePacket(close); err != nil {
					t.Fatal(err)
				}
				close.ContainerType = 0
				if got, want := takeContainerTestPacket(t, session), encodeTestPacket(close); !bytes.Equal(got, want) {
					t.Fatalf("translated server close = %x, want %x", got, want)
				}

				// A later close must not inherit the retired translation.
				clientClose := &packet.ContainerClose{WindowID: open.WindowID, ContainerType: 0}
				if got, want := readContainerTestClose(t, session, clientClose), encodeTestPacket(clientClose); !bytes.Equal(got, want) {
					t.Fatalf("close after retirement = %x, want %x", got, want)
				}
			})
		}
	}
}

func TestInventoryDiagnosticsBoundContentWithoutDroppingRawPacket(t *testing.T) {
	previous := slog.Default()
	t.Cleanup(func() { slog.SetDefault(previous) })
	for _, test := range []struct {
		name   string
		slots  int
		logged bool
	}{
		{name: "ordinary", slots: 36, logged: true},
		{name: "oversized", slots: 8192},
	} {
		t.Run(test.name, func(t *testing.T) {
			var logs bytes.Buffer
			slog.SetDefault(slog.New(slog.NewTextHandler(&logs, nil)))
			session := containerTestSession()
			data := encodeTestPacket(&packet.InventoryContent{Content: make([]protocol.ItemInstance, test.slots)})
			if err := session.WritePacketRaw(data); err != nil {
				t.Fatal(err)
			}
			if got := takeContainerTestPacket(t, session); !bytes.Equal(got, data) {
				t.Fatal("diagnostic decoding changed the raw inventory packet")
			}
			if logged := strings.Contains(logs.String(), "slots_count="); logged != test.logged {
				t.Fatalf("decoded inventory diagnostic = %t, want %t", logged, test.logged)
			}
		})
	}
}

// containerTestSession keeps both relay directions in memory without starting a transport reader.
func containerTestSession() *sessionConn {
	return &sessionConn{
		frames:         make(chan sessionRead, 1),
		clientPool:     minecraft.DefaultProtocol.Packets(true),
		serverPool:     minecraft.DefaultProtocol.Packets(false),
		virtualWindows: make(map[byte]bool),
	}
}

// takeContainerTestPacket consumes exactly one packet queued for the client.
func takeContainerTestPacket(t *testing.T, session *sessionConn) []byte {
	t.Helper()
	if len(session.pending) == 0 {
		t.Fatal("no packet queued for the client")
	}
	packets, err := splitSessionBatch(session.pending[1:])
	if err != nil {
		t.Fatal(err)
	}
	if len(packets) != 1 {
		t.Fatalf("queued %d packets, want one", len(packets))
	}
	session.pending = nil
	return packets[0]
}

// readContainerTestClose returns the server-bound bytes for a client close.
func readContainerTestClose(t *testing.T, session *sessionConn, close *packet.ContainerClose) []byte {
	t.Helper()
	frame := appendSessionBatchPacket([]byte{bridgecontract.SessionKindBatch}, encodeTestPacket(close))
	session.frames <- sessionRead{frame: frame}
	packets, err := session.ReadBatchRaw(func(id uint32) bool { return id == packet.IDContainerClose })
	if err != nil {
		t.Fatal(err)
	}
	if len(packets) != 1 {
		t.Fatalf("read %d packets, want one", len(packets))
	}
	return packets[0].Data
}
