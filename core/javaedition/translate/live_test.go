package translate

import (
	"bytes"
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// liveServerEnv names an offline-mode vanilla 1.7.10 server, as host:port, for the live test.
const liveServerEnv = "CINNABAR_JAVA_1_7_10_SERVER"

// serverPackets constructs the Bedrock packets a server sends, by id.
var serverPackets = packet.NewServerPool()

// decodeServerPacket decodes a translated packet the way a Bedrock client's codec would.
func decodeServerPacket(t *testing.T, data []byte) packet.Packet {
	t.Helper()
	buf := bytes.NewBuffer(data)
	var header packet.Header
	if err := header.Read(buf); err != nil {
		t.Fatalf("read header: %v", err)
	}
	construct, ok := serverPackets[header.PacketID]
	if !ok {
		t.Fatalf("packet %d is not a server packet", header.PacketID)
	}
	pk := construct()
	func() {
		defer func() {
			if recovered := recover(); recovered != nil {
				t.Fatalf("decode packet %d: %v", header.PacketID, recovered)
			}
		}()
		pk.Marshal(protocol.NewReader(buf, 0, true))
	}()
	if buf.Len() != 0 {
		t.Fatalf("packet %d left %d bytes unread", header.PacketID, buf.Len())
	}
	return pk
}

// drain reads translated batches on its own goroutine, as the session relay does, and
// delivers the decoded packets in order. The channel closes when the session ends.
func drain(t *testing.T, session *Session) <-chan packet.Packet {
	t.Helper()
	packets := make(chan packet.Packet, 4096)
	go func() {
		defer close(packets)
		for {
			batch, err := session.ReadBatchRaw(nil)
			if err != nil {
				return
			}
			for _, raw := range batch {
				packets <- decodeServerPacket(t, raw.Data)
			}
		}
	}()
	return packets
}

// readUntil takes packets until match accepts one, failing after timeout.
func readUntil(t *testing.T, packets <-chan packet.Packet, timeout time.Duration, match func(packet.Packet) bool) []packet.Packet {
	t.Helper()
	deadline := time.NewTimer(timeout)
	defer deadline.Stop()
	var seen []packet.Packet
	for {
		select {
		case pk, ok := <-packets:
			if !ok {
				t.Fatalf("the session ended after %d packets", len(seen))
			}
			seen = append(seen, pk)
			if match(pk) {
				return seen
			}
		case <-deadline.C:
			t.Fatalf("no matching packet among %d within %s", len(seen), timeout)
		}
	}
}

// The translator joins a real 1.7.10 server, receives its world, walks and chats.
func TestLiveVanillaServer(t *testing.T) {
	address := os.Getenv(liveServerEnv)
	if address == "" {
		t.Skipf("missing fixture: set %s to an offline-mode vanilla 1.7.10 server", liveServerEnv)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	session, err := Dial(ctx, address, Options{Username: "CinnabarLive"})
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	packets := drain(t, session)
	const patience = 20 * time.Second
	first := readUntil(t, packets, patience, func(packet.Packet) bool { return true })
	if _, ok := first[0].(*packet.StartGame); !ok {
		t.Fatalf("first packet is %T, want StartGame", first[0])
	}
	if err := session.WritePacket(&packet.RequestChunkRadius{ChunkRadius: 6}); err != nil {
		t.Fatal(err)
	}
	startup := readUntil(t, packets, patience, func(pk packet.Packet) bool {
		status, ok := pk.(*packet.PlayStatus)
		return ok && status.Status == packet.PlayStatusPlayerSpawn
	})
	var spawn *packet.MovePlayer
	for _, pk := range startup {
		if move, ok := pk.(*packet.MovePlayer); ok {
			spawn = move
		}
	}
	if spawn == nil {
		t.Fatal("the server sent no position before the spawn status")
	}
	// The server streams the columns around the player after placing it.
	home := protocol.ChunkPos{int32(floor(float64(spawn.Position[0]))) >> 4, int32(floor(float64(spawn.Position[2]))) >> 4}
	var floorBlocks int
	world := readUntil(t, packets, patience, func(pk packet.Packet) bool {
		chunk, ok := pk.(*packet.LevelChunk)
		if ok && chunk.Position == home {
			floorBlocks = len(chunk.RawPayload)
		}
		return ok && chunk.Position == home
	})
	t.Logf("startup: %d packets, spawn at %v, home column payload %d bytes after %d world packets", len(startup), spawn.Position, floorBlocks, len(world))

	// Walk a few blocks along X on the flat world, one input per tick.
	position := spawn.Position
	// The server paces movement in real time: one input per 50 ms tick.
	ticks := time.NewTicker(50 * time.Millisecond)
	defer ticks.Stop()
	for tick := uint64(0); tick < 40; tick++ {
		position = position.Add(mgl32.Vec3{0.1, 0, 0})
		input := &packet.PlayerAuthInput{Position: position, Yaw: spawn.Yaw, Pitch: spawn.Pitch, Tick: tick, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		input.InputData.Set(packet.InputFlagVerticalCollision)
		if err := session.WritePacket(input); err != nil {
			t.Fatal(err)
		}
		if err := session.Flush(); err != nil {
			t.Fatal(err)
		}
		<-ticks.C
	}
	if err := session.WritePacket(&packet.Text{TextType: packet.TextTypeChat, Message: "hello from cinnabar"}); err != nil {
		t.Fatal(err)
	}
	if err := session.Flush(); err != nil {
		t.Fatal(err)
	}
	after := readUntil(t, packets, patience, func(pk packet.Packet) bool {
		text, ok := pk.(*packet.Text)
		return ok && strings.Contains(text.Message, "hello from cinnabar")
	})
	for _, pk := range after {
		if move, ok := pk.(*packet.MovePlayer); ok {
			t.Fatalf("the server corrected the walk back to %v", move.Position)
		}
	}
	t.Logf("chat echoed after %d packets: %q", len(after), after[len(after)-1].(*packet.Text).Message)
}
