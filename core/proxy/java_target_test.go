package proxy

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestJavaTargets(t *testing.T) {
	const prefix = JavaTargetPrefix
	if !IsJavaTarget(prefix+"play.example.net") || !IsJavaTarget(" "+strings.ToUpper(prefix)+"play.example.net:25565") || IsJavaTarget("play.example.net:19132") {
		t.Fatal("IsJavaTarget misjudged a target")
	}
	target, err := resolveUpstreamTarget(context.Background(), prefix+"play.example.net", nil, nil, nil)
	if err != nil || !target.java || target.address != "play.example.net" {
		t.Fatalf("target = %+v, %v", target, err)
	}
	if !ValidJavaTarget(prefix + "play.example.net:25565") {
		t.Error("a Java target with a port is invalid")
	}
	for _, invalid := range []string{prefix, prefix + "two words", prefix + "a/b"} {
		if _, err := resolveUpstreamTarget(context.Background(), invalid, nil, nil, nil); err == nil {
			t.Errorf("%q resolved", invalid)
		}
		if ValidJavaTarget(invalid) {
			t.Errorf("%q is valid", invalid)
		}
	}
}

// readJavaPacket reads serverbound packets until one of type T arrives.
func readJavaPacket[T java.Packet](t *testing.T, client *javatest.Client) T {
	t.Helper()
	for {
		pk, err := client.Read()
		if err != nil {
			t.Fatalf("read from the Java client: %v", err)
		}
		if match, ok := pk.(T); ok {
			return match
		}
	}
}

// A Java target joins through the same session handoff and relay as a Bedrock server: the
// client receives StartGame in the handoff, translated packets in batches, and its own
// packets reach the server as Java packets.
func TestSessionServerJoinsAJavaServer(t *testing.T) {
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	dir := t.TempDir()
	newTestSessionServer(t, dir, func(sessions *sessionServer) {
		sessions.prepared.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
			return javaTarget(server.Addr())
		}
	})
	joined := make(chan *javatest.Client, 1)
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		client, err := server.Accept(ctx)
		if err != nil {
			joined <- nil
			return
		}
		_ = client.Write(&java.JoinGame{EntityID: 7, GameMode: 1, MaxPlayers: 20, LevelType: "flat"})
		joined <- client
	}()

	// The client claims the target it joins as its login server address.
	request := testSessionConnect(t)
	var claims map[string]any
	if err := json.Unmarshal(request.ClientData, &claims); err != nil {
		t.Fatal(err)
	}
	claims["ServerAddress"] = JavaTargetPrefix + server.Addr()
	if request.ClientData, err = json.Marshal(claims); err != nil {
		t.Fatal(err)
	}
	client, frames := dialTestSession(t, dir, request)
	handoffFrame := <-frames
	if handoffFrame[0] != bridgecontract.SessionKindHandoff {
		t.Fatalf("first frame kind %d", handoffFrame[0])
	}
	length := binary.BigEndian.Uint32(handoffFrame[1:5])
	var handoff sessionHandoff
	if err := json.Unmarshal(handoffFrame[5:5+length], &handoff); err != nil {
		t.Fatal(err)
	}
	startup, err := splitSessionBatch(handoffFrame[5+length:])
	if err != nil {
		t.Fatal(err)
	}
	if len(startup) != 1 || startup[0][0] != packet.IDStartGame {
		t.Fatalf("startup holds %d packets", len(startup))
	}
	if handoff.PacksRequired || len(handoff.Packs) != 0 || handoff.Identity.DisplayName != "Steve" {
		t.Fatalf("handoff = %+v", handoff)
	}

	javaClient := <-joined
	if javaClient == nil {
		t.Fatal("the Java server accepted no login")
	}
	if javaClient.Username != "Steve" {
		t.Fatalf("logged in as %q", javaClient.Username)
	}
	if settings := readJavaPacket[*java.ClientSettings](t, javaClient); settings.Locale != "en_US" {
		t.Fatalf("settings = %+v", settings)
	}

	// The item registry shares StartGame's batch and follows the handoff.
	rest := <-frames
	packets, err := splitSessionBatch(rest[1:])
	if rest[0] != bridgecontract.SessionKindBatch || err != nil || len(packets) != 1 {
		t.Fatalf("frame after the handoff: kind %d, %d packets, %v", rest[0], len(packets), err)
	}
	if header, _ := binary.Uvarint(packets[0]); header&0x3ff != packet.IDItemRegistry {
		t.Fatalf("packet %d follows the handoff, want the item registry", header&0x3ff)
	}

	_ = javaClient.Write(&java.ChatMessage{Message: `{"text":"welcome"}`})
	batch := <-frames
	packets, err = splitSessionBatch(batch[1:])
	if batch[0] != bridgecontract.SessionKindBatch || err != nil || len(packets) != 1 || packets[0][0] != packet.IDText {
		t.Fatalf("relayed frame = %x", batch)
	}

	chat := encodeTestPacket(&packet.Text{TextType: packet.TextTypeChat, Message: "hi"})
	if _, err := client.Write(append([]byte{bridgecontract.SessionKindBatch}, appendSessionBatchPacket(nil, chat)...)); err != nil {
		t.Fatal(err)
	}
	if message := readJavaPacket[*java.ClientChatMessage](t, javaClient); message.Message != "hi" {
		t.Fatalf("the server read chat %q", message.Message)
	}
}
