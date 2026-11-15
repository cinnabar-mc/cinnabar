package translate

import (
	"bytes"
	"compress/zlib"
	"context"
	"encoding/binary"
	"encoding/json"
	"image/color"
	"math"
	"os"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/skins/skinstest"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Fixtures shared with the client's decoders.
const (
	sessionFixture      = "../../../crates/protocol/fixtures/java_session.bin"
	registryFixture     = "../../../crates/protocol/fixtures/java_item_registry.bin"
	chunkPayloadFixture = "../../../crates/world/tests/fixtures/java_level_chunk.bin"
	chunkBlocksFixture  = "../../../crates/world/tests/fixtures/java_level_chunk.json"
)

// batchHeader starts an uncompressed Bedrock packet batch, the form the client's fixture
// decoder reads.
const batchHeader = 0xfe

// testSession is a translated session joined to a fake 1.7.10 server.
type testSession struct {
	t       *testing.T
	session *Session
	server  *javatest.Client
}

func startTestSession(t *testing.T) *testSession {
	t.Helper()
	return startTestSessionWith(t, Options{})
}

// startTestSessionWith joins a fake server as Steve with options.
func startTestSessionWith(t *testing.T, options Options) *testSession {
	t.Helper()
	options.Username = "Steve"
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	session, err := Dial(ctx, server.Addr(), options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = session.Close() })
	client, err := server.Accept(ctx)
	if err != nil {
		t.Fatal(err)
	}
	return &testSession{t: t, session: session, server: client}
}

// serve sends a server packet and returns what it translated to. Every packet here
// translates to at least one Bedrock packet.
func (ts *testSession) serve(pk java.Packet) []packet.Packet {
	ts.t.Helper()
	if err := ts.server.Write(pk); err != nil {
		ts.t.Fatal(err)
	}
	return ts.batch()
}

// batch reads the next translated batch, decoded as a Bedrock client would.
func (ts *testSession) batch() []packet.Packet {
	ts.t.Helper()
	batch, err := ts.session.ReadBatchRaw(nil)
	if err != nil {
		ts.t.Fatal(err)
	}
	packets := make([]packet.Packet, len(batch))
	for i, raw := range batch {
		packets[i] = decodeServerPacket(ts.t, raw.Data)
	}
	return packets
}

// expect reads serverbound packets until one of type T arrives.
func expect[T java.Packet](ts *testSession) T {
	ts.t.Helper()
	for {
		pk, err := ts.server.Read()
		if err != nil {
			ts.t.Fatalf("read from the translator: %v", err)
		}
		if match, ok := pk.(T); ok {
			return match
		}
	}
}

// write sends a client packet through the translator and flushes it.
func (ts *testSession) write(pk packet.Packet) {
	ts.t.Helper()
	data, err := encodeBedrock(pk)
	if err != nil {
		ts.t.Fatal(err)
	}
	if err := ts.session.WritePacketRaw(data); err != nil {
		ts.t.Fatal(err)
	}
	if err := ts.session.Flush(); err != nil {
		ts.t.Fatal(err)
	}
}

// compress deflates chunk data the way the server sends it.
func compress(t *testing.T, data []byte) []byte {
	t.Helper()
	var out bytes.Buffer
	writer := zlib.NewWriter(&out)
	if _, err := writer.Write(data); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return out.Bytes()
}

// fixtureColumn is the column the shared fixtures describe: a stone floor with a fence
// between stone and air, stairs, water, red wool and one desert biome cell.
func fixtureColumn() *javaColumn {
	column := newJavaColumn()
	for x := 0; x < 16; x++ {
		for z := 0; z < 16; z++ {
			column.set(x, 0, z, 1, 0)
		}
	}
	column.set(1, 1, 1, blockFence, 0)
	column.set(2, 1, 1, 1, 0)
	column.set(5, 1, 5, 53, 0)
	column.set(8, 1, 8, 9, 0)
	column.set(9, 17, 9, 35, 14)
	for i := range column.biomes {
		column.biomes[i] = 1
	}
	column.biomes[0] = 2
	return column
}

// checkFixture compares data with a golden file, rewriting it when asked to.
func checkFixture(t *testing.T, path string, data []byte) {
	t.Helper()
	if os.Getenv(updateFixturesEnv) != "" {
		if err := os.WriteFile(path, data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	current, err := os.ReadFile(path)
	if err != nil {
		t.Skipf("missing fixture: %s", path)
	}
	if !bytes.Equal(current, data) {
		t.Fatalf("%s is stale; rerun with %s=1", path, updateFixturesEnv)
	}
}

// checkRegistryFixture compares the item registry with its golden packet by content: the
// component data is NBT compounds, whose field order on the wire is not fixed.
func checkRegistryFixture(t *testing.T, registry *packet.ItemRegistry) {
	t.Helper()
	if os.Getenv(updateFixturesEnv) != "" {
		data, err := encodeBedrock(registry)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(registryFixture, append(binary.AppendUvarint([]byte{batchHeader}, uint64(len(data))), data...), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	current, err := os.ReadFile(registryFixture)
	if err != nil {
		t.Skipf("missing fixture: %s", registryFixture)
	}
	if len(current) == 0 || current[0] != batchHeader {
		t.Fatalf("%s is not a packet batch", registryFixture)
	}
	current = current[1:]
	length, prefix := binary.Uvarint(current)
	if prefix <= 0 || int(length) != len(current)-prefix {
		t.Fatalf("%s is not one length-prefixed packet", registryFixture)
	}
	golden, ok := decodeServerPacket(t, current[prefix:]).(*packet.ItemRegistry)
	if !ok || !reflect.DeepEqual(golden.Items, registry.Items) {
		t.Fatalf("%s is stale; rerun with %s=1", registryFixture, updateFixturesEnv)
	}
}

// A scripted join produces a fixed packet stream. The stream and the column's payload are
// golden files that the client's own decoders read in their tests.
func TestSessionTranslationMatchesTheSharedFixtures(t *testing.T) {
	services := skinstest.Start(t)
	ts := startTestSessionWith(t, Options{Skins: services.Fetcher()})
	var stream []packet.Packet
	serve := func(pk java.Packet) []packet.Packet {
		packets := ts.serve(pk)
		stream = append(stream, packets...)
		return packets
	}

	joined := serve(&java.JoinGame{EntityID: 7, GameMode: 0, Difficulty: 2, MaxPlayers: 20, LevelType: "default"})
	start, ok := joined[0].(*packet.StartGame)
	if !ok || len(joined) != 2 {
		t.Fatalf("join translated to %d packets, first %T", len(joined), joined[0])
	}
	if start.EntityRuntimeID != runtimeID(7) || start.EntityUniqueID != uniqueID(7) || !start.UseBlockNetworkIDHashes || start.Dimension != bedrockOverworld {
		t.Fatalf("StartGame = runtime %d, hashes %t, dimension %d", start.EntityRuntimeID, start.UseBlockNetworkIDHashes, start.Dimension)
	}
	if registry, ok := joined[1].(*packet.ItemRegistry); !ok || len(registry.Items) != len(legacy.Registry()) {
		t.Fatalf("second packet is %T", joined[1])
	}
	if settings := expect[*java.ClientSettings](ts); settings.ViewDistance != 8 {
		t.Fatalf("settings = %+v", settings)
	}
	if brand := expect[*java.ClientPluginMessage](ts); brand.Channel != "MC|Brand" || string(brand.Data) != "vanilla" {
		t.Fatalf("brand = %+v", brand)
	}

	primary, add, data := fixtureColumn().data()
	chunks := serve(&java.ChunkData{ChunkX: 0, ChunkZ: 0, GroundUpContinuous: true, PrimaryBitmask: primary, AddBitmask: add, Compressed: compress(t, data)})
	chunk, ok := chunks[0].(*packet.LevelChunk)
	if !ok || len(chunks) != 1 {
		t.Fatalf("chunk translated to %d packets, first %T", len(chunks), chunks[0])
	}
	checkFixture(t, chunkPayloadFixture, chunk.RawPayload)

	spawn := serve(&java.PlayerPositionAndLook{X: 8.5, Y: 1 + eyeHeight, Z: 8.5, Yaw: 90, Pitch: 10, OnGround: true})
	move, ok := spawn[1].(*packet.MovePlayer)
	if _, publisher := spawn[0].(*packet.NetworkChunkPublisherUpdate); !publisher || !ok {
		t.Fatalf("spawn translated to %T, %T", spawn[0], spawn[1])
	}
	if move.Mode != packet.MoveModeTeleport || move.Position != (mgl32.Vec3{8.5, float32(1 + networkOffset), 8.5}) || move.Yaw != 90 {
		t.Fatalf("MovePlayer = %+v", move)
	}
	if status, ok := spawn[len(spawn)-1].(*packet.PlayStatus); !ok || status.Status != packet.PlayStatusPlayerSpawn {
		t.Fatalf("spawn ends with %T", spawn[len(spawn)-1])
	}
	// The client echoes a teleport with the packet's own values.
	echo := expect[*java.ClientPlayerPositionLook](ts)
	want := &java.ClientPlayerPositionLook{X: 8.5, Y: 1 + eyeHeight - eyeHeight, Stance: 1 + eyeHeight, Z: 8.5, Yaw: 90, Pitch: 10, OnGround: true}
	if !reflect.DeepEqual(echo, want) {
		t.Fatalf("teleport echo = %+v, want %+v", echo, want)
	}

	serve(&java.TimeUpdate{WorldAge: 100, TimeOfDay: -6000})
	chat := serve(&java.ChatMessage{Message: `{"translate":"chat.type.text","with":["Alex",{"text":"hi","color":"red"}]}`})
	if text := chat[0].(*packet.Text); text.Message != "<Alex> §r§chi§r" {
		t.Fatalf("chat = %q", text.Message)
	}
	serve(&java.UpdateHealth{Health: 15, Food: 18, Saturation: 2.5})
	textures := skinstest.Payload(services.Texture("skin", 64, 32, color.NRGBA{R: 0xc0, G: 0x80, B: 0x40, A: 0xff}), services.Texture("cape", 64, 32, color.NRGBA{B: 0xc0, A: 0xff}))
	serve(&java.SpawnPlayer{EntityID: 9, UUID: "00000000-0000-4000-8000-000000000009", Name: "Alex", X: 8 * 32, Y: 1 * 32, Z: 4 * 32, Yaw: 64, CurrentItem: 0,
		Properties: []java.ProfileProperty{services.Signed(textures)},
		Metadata:   java.Metadata{{Index: 0, Type: java.MetadataByte, Value: int8(flagSneaking)}}})
	// The signed skin and cape follow once downloaded, in the spawn's batch if the download
	// finished before it was flushed.
	if last := stream[len(stream)-1]; last.ID() != packet.IDPlayerSkin {
		stream = append(stream, ts.skinAfter(nil))
	}
	// A player with a cape over the default skin gets it without skin pixels.
	capeOnly := skinstest.Payload("", services.Texture("cape", 64, 32, color.NRGBA{B: 0xc0, A: 0xff}))
	serve(&java.SpawnPlayer{EntityID: 10, UUID: "00000000-0000-4000-8000-00000000000a", Name: "Notch", X: 12 * 32, Y: 1 * 32, Z: 4 * 32,
		Properties: []java.ProfileProperty{services.Signed(capeOnly)}})
	if last := stream[len(stream)-1]; last.ID() != packet.IDPlayerSkin {
		stream = append(stream, ts.skinAfter(nil))
	}
	moved := serve(&java.EntityRelativeMove{EntityID: 9, DX: 16, DY: 0, DZ: -32})
	if actor := moved[0].(*packet.MoveActorAbsolute); actor.Position != (mgl32.Vec3{8.5, float32(1 + networkOffset), 3}) {
		t.Fatalf("remote player moved to %v", actor.Position)
	}
	// Stone beside the fence joins it on that side too.
	placed := serve(&java.BlockChange{X: 0, Y: 1, Z: 1, BlockID: 1})
	if len(placed) != 2 {
		t.Fatalf("a block beside a fence translated to %d updates", len(placed))
	}
	fence := placed[1].(*packet.UpdateBlock)
	if fence.Position != (protocol.BlockPos{1, 1, 1}) || fence.NewBlockRuntimeID == legacy.BlockHash(blockFence, 0) {
		t.Fatalf("fence update = %+v", fence)
	}
	sword := serve(&java.SetSlot{WindowID: 0, Slot: 36, Item: java.ItemStack{ID: 276, Count: 1, Damage: 12}})
	slot := sword[0].(*packet.InventorySlot)
	if id, _ := legacy.ItemID("minecraft:diamond_sword"); slot.Slot != 0 || slot.NewItem.Stack.NetworkID != int32(id) || slot.NewItem.Stack.NBTData["Damage"] != int32(12) {
		t.Fatalf("sword slot = %+v", slot)
	}
	serve(&java.DestroyEntities{EntityIDs: []int32{9}})

	encoded := []byte{batchHeader}
	for _, pk := range stream {
		if registry, ok := pk.(*packet.ItemRegistry); ok {
			checkRegistryFixture(t, registry)
			continue
		}
		data, err := encodeBedrock(pk)
		if err != nil {
			t.Fatal(err)
		}
		encoded = binary.AppendUvarint(encoded, uint64(len(data)))
		encoded = append(encoded, data...)
	}
	checkFixture(t, sessionFixture, encoded)

	// The blocks the client's chunk decoder should find in the payload.
	w := filledWorld(t, map[chunkPos]*javaColumn{{}: fixtureColumn()})
	type cell struct {
		X     int32  `json:"x"`
		Y     int32  `json:"y"`
		Z     int32  `json:"z"`
		ID    uint32 `json:"network_id"`
		State string `json:"state"`
	}
	var cells []cell
	for _, position := range [][3]int32{{0, 0, 0}, {15, 0, 15}, {1, 1, 1}, {2, 1, 1}, {5, 1, 5}, {8, 1, 8}, {9, 17, 9}, {3, 1, 3}, {0, 40, 0}} {
		cells = append(cells, cell{position[0], position[1], position[2], w.networkID(position[0], position[1], position[2]), w.state(position[0], position[1], position[2]).String()})
	}
	blocks, err := json.MarshalIndent(map[string]any{"sub_chunk_count": chunk.SubChunkCount, "blocks": cells, "biome_at_origin": 2, "biome_elsewhere": 1}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	checkFixture(t, chunkBlocksFixture, append(blocks, '\n'))
}

func TestClientPacketsBecomeJavaPackets(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	// The 1.7.10 client sends no movement while its column is not loaded.
	ts.loadColumn(0, 0)
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)

	ts.write(&packet.RequestChunkRadius{ChunkRadius: 12})
	if settings := expect[*java.ClientSettings](ts); settings.ViewDistance != 12 {
		t.Fatalf("settings = %+v", settings)
	}
	if radius := ts.batch()[0].(*packet.ChunkRadiusUpdated); radius.ChunkRadius != 12 {
		t.Fatalf("radius = %+v", radius)
	}

	input := func(position mgl32.Vec3, yaw float32, flags ...int) *packet.PlayerAuthInput {
		pk := &packet.PlayerAuthInput{Position: position, Yaw: yaw, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		for _, flag := range flags {
			pk.InputData.Set(flag)
		}
		return pk
	}
	eye := float32(64 + networkOffset)
	// The first tick reports position and look; standing still then reports only the ground.
	ts.write(input(mgl32.Vec3{0.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision))
	first := expect[*java.ClientPlayerPositionLook](ts)
	if first.X != 0.5 || first.Z != 0.5 || first.Yaw != 30 || !first.OnGround || first.Stance-first.Y != eyeHeight || math.Abs(first.Y-64) > 1e-5 {
		t.Fatalf("first movement = %+v", first)
	}
	ts.write(input(mgl32.Vec3{0.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision))
	if idle := expect[*java.ClientPlayer](ts); !idle.OnGround {
		t.Fatalf("idle = %+v", idle)
	}
	// Holding the sprint key is not sprinting: only a start tells the server the player sprints.
	ts.write(input(mgl32.Vec3{0.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision, packet.InputFlagSprinting))
	if idle := expect[*java.ClientPlayer](ts); !idle.OnGround {
		t.Fatalf("holding the sprint key sent %+v", idle)
	}
	// Sprinting is announced before the movement it applies to.
	ts.write(input(mgl32.Vec3{1.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision, packet.InputFlagStartSprinting))
	if action := expect[*java.ClientEntityAction](ts); action.EntityID != 7 || action.Action != actionStartSprinting {
		t.Fatalf("action = %+v", action)
	}
	if moved := expect[*java.ClientPlayerPosition](ts); moved.X != 1.5 {
		t.Fatalf("moved = %+v", moved)
	}

	ts.write(&packet.Animate{ActionType: packet.AnimateActionSwingArm, EntityRuntimeID: 7})
	if swing := expect[*java.ClientAnimation](ts); swing.EntityID != 7 || swing.Animation != 1 {
		t.Fatalf("swing = %+v", swing)
	}
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.UseItemOnEntityTransactionData{TargetEntityRuntimeID: runtimeID(9), ActionType: protocol.UseItemOnEntityActionAttack}})
	if attack := expect[*java.ClientUseEntity](ts); attack.EntityID != 9 || attack.Action != useEntityAttack {
		t.Fatalf("attack = %+v", attack)
	}
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.UseItemTransactionData{
		ActionType: protocol.UseItemActionClickBlock, BlockPosition: protocol.BlockPos{3, 63, -2}, BlockFace: 1, ClickedPosition: mgl32.Vec3{0.5, 1, 0.25},
	}})
	if place := expect[*java.ClientBlockPlacement](ts); place.X != 3 || place.Y != 63 || place.Z != -2 || place.Face != 1 || place.CursorX != 8 || place.CursorY != 16 || place.CursorZ != 4 {
		t.Fatalf("placement = %+v", place)
	}
	// A new slot is reported at the next tick's sync, before its movement, and only once.
	ts.write(&packet.MobEquipment{HotBarSlot: 4, WindowID: protocol.WindowIDInventory})
	ts.write(&packet.MobEquipment{HotBarSlot: 4, WindowID: protocol.WindowIDInventory})
	ts.write(input(mgl32.Vec3{1.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision))
	if next, err := ts.server.Read(); err != nil {
		t.Fatal(err)
	} else if held, ok := next.(*java.ClientHeldItemChange); !ok || held.Slot != 4 {
		t.Fatalf("the tick after a slot change first sent %+v", next)
	}
	if next, err := ts.server.Read(); err != nil {
		t.Fatal(err)
	} else if _, ok := next.(*java.ClientHeldItemChange); ok {
		t.Fatalf("the slot was reported twice")
	}
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: "hello"})
	if chat := expect[*java.ClientChatMessage](ts); chat.Message != "hello" {
		t.Fatalf("chat = %+v", chat)
	}
	breaking := input(mgl32.Vec3{1.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision, packet.InputFlagSprinting)
	breaking.BlockActions = protocol.Option([]protocol.PlayerBlockAction{
		{Action: protocol.PlayerActionStartBreak, BlockPos: protocol.BlockPos{1, 63, 0}, Face: 1},
		{Action: protocol.PlayerActionPredictDestroyBlock, BlockPos: protocol.BlockPos{1, 63, 0}, Face: 1},
	})
	breaking.InputData.Set(packet.InputFlagPerformBlockActions)
	ts.write(breaking)
	// The tick's mining goes out before its movement, as Minecraft.runTick sends it.
	tickFirst, err := ts.server.Read()
	if err != nil {
		t.Fatal(err)
	}
	if dig, ok := tickFirst.(*java.ClientPlayerDigging); !ok || dig.Status != digStart || dig.X != 1 || dig.Y != 63 || dig.Face != 1 {
		t.Fatalf("the tick's first packet = %+v, want the dig start", tickFirst)
	}
	if dig := expect[*java.ClientPlayerDigging](ts); dig.Status != digFinish {
		t.Fatalf("dig finish = %+v", dig)
	}
}

// A client that has not applied a server teleport yet still reports its old position; the
// 1.7.10 server would treat that as movement from the teleport target.
func TestMovementWaitsForTheClientToReachATeleport(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	// The 1.7.10 client sends no movement while its column is not loaded.
	ts.loadColumn(0, 0)
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	eye := float32(64 + networkOffset)
	stale := &packet.PlayerAuthInput{Position: mgl32.Vec3{100, eye, 100}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	ts.write(stale)
	arrived := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	ts.write(arrived)
	if first := expect[*java.ClientPlayerPosition](ts); first.X != 0.5 || first.Z != 0.5 {
		t.Fatalf("the first forwarded movement is %+v", first)
	}
}

func TestServerDisconnectReachesTheClient(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	kicked := ts.serve(&java.Disconnect{Reason: `{"text":"Kicked","color":"red"}`})
	if disconnect := kicked[0].(*packet.Disconnect); disconnect.Message != "§r§cKicked" {
		t.Fatalf("disconnect = %+v", disconnect)
	}
}

// Both directions flush translated packets. A batch taken first must reach the client first
// even when the client is slow: a chunk overtaken by a later block change would undo it.
func TestConcurrentFlushesKeepTheirOrder(t *testing.T) {
	session := &Session{batches: make(chan []outbound, 1), closed: make(chan struct{})}
	text := func(message string) *packet.Text { return &packet.Text{TextType: packet.TextTypeRaw, Message: message} }
	session.queue(text("first"))
	session.flushPending() // fills the queue, so the next flush has to wait

	done := make(chan struct{}, 2)
	session.queue(text("second"))
	go func() { session.flushPending(); done <- struct{}{} }()
	// The second flush holds the order lock while it waits for room.
	testwait.Eventually(t, 5*time.Second, "the second flush to take its batch", func() bool {
		session.pendingMu.Lock()
		defer session.pendingMu.Unlock()
		return len(session.pending) == 0
	})
	session.queue(text("third"))
	go func() { session.flushPending(); done <- struct{}{} }()

	for _, want := range []string{"first", "second", "third"} {
		batch, err := session.ReadBatchRaw(nil)
		if err != nil {
			t.Fatal(err)
		}
		if got := decodeServerPacket(t, batch[0].Data).(*packet.Text).Message; got != want {
			t.Fatalf("read %q, want %q", got, want)
		}
	}
	<-done
	<-done
}

// A column the server unloads is emptied for the client, and connections into it let go.
func TestUnloadedColumnsAreClearedForTheClient(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	send := func(x int32, column *javaColumn) []packet.Packet {
		primary, add, data := column.data()
		return ts.serve(&java.ChunkData{ChunkX: x, GroundUpContinuous: true, PrimaryBitmask: primary, AddBitmask: add, Compressed: compress(t, data)})
	}
	// A fence on the east edge of column 0 joins stone on the west edge of column 1.
	west := newJavaColumn()
	west.set(15, 1, 3, blockFence, 0)
	send(0, west)
	east := newJavaColumn()
	east.set(0, 1, 3, 1, 0)
	joined := send(1, east)
	connected := joined[len(joined)-1].(*packet.UpdateBlock)
	if connected.Position != (protocol.BlockPos{15, 1, 3}) || connected.NewBlockRuntimeID == legacy.BlockHash(blockFence, 0) {
		t.Fatalf("loading the neighbour sent %+v", connected)
	}

	unloaded := ts.serve(&java.ChunkData{ChunkX: 1, GroundUpContinuous: true})
	cleared, ok := unloaded[0].(*packet.LevelChunk)
	if !ok || cleared.Position != (protocol.ChunkPos{1, 0}) || cleared.SubChunkCount != 4 {
		t.Fatalf("unload translated to %+v", unloaded[0])
	}
	released := unloaded[len(unloaded)-1].(*packet.UpdateBlock)
	if released.Position != (protocol.BlockPos{15, 1, 3}) || released.NewBlockRuntimeID != legacy.BlockHash(blockFence, 0) {
		t.Fatalf("the fence beside the unloaded column became %+v", released)
	}
	// Changes inside an unloaded column are ignored, as the 1.7.10 client ignores them.
	ts.serve(&java.ChatMessage{Message: `"marker"`})
}

func TestBulkColumnsArriveOneBatchEach(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	column := newJavaColumn()
	column.set(0, 0, 0, 1, 0)
	primary, add, data := column.data()
	bulk := &java.MapChunkBulk{SkyLight: true, Compressed: compress(t, append(append([]byte{}, data...), data...)), Columns: []java.ChunkColumn{
		{ChunkX: 0, ChunkZ: 0, PrimaryBitmask: primary, AddBitmask: add},
		{ChunkX: 5, ChunkZ: -2, PrimaryBitmask: primary, AddBitmask: add},
	}}
	first := ts.serve(bulk)
	second := ts.batch()
	for index, batch := range [][]packet.Packet{first, second} {
		chunk, ok := batch[0].(*packet.LevelChunk)
		if !ok || len(batch) != 1 || chunk.Position != [2]int32{bulk.Columns[index].ChunkX, bulk.Columns[index].ChunkZ} {
			t.Fatalf("bulk column %d translated to %+v", index, batch)
		}
	}
}

// A respawn in the same dimension keeps the world the 1.7.10 client keeps; a change of
// dimension replaces it.
func TestRespawnKeepsTheWorldUnlessTheDimensionChanges(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	column := newJavaColumn()
	column.set(0, 0, 0, 1, 0)
	primary, add, data := column.data()
	ts.serve(&java.ChunkData{GroundUpContinuous: true, PrimaryBitmask: primary, AddBitmask: add, Compressed: compress(t, data)})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 1 + eyeHeight, Z: 0.5, OnGround: true})
	ts.serve(&java.SpawnMob{EntityID: 9, Type: 90})

	respawned := ts.serve(&java.Respawn{Dimension: 0, LevelType: "default"})
	// The new player's movement speed goes back to the default, then its game mode.
	if len(respawned) != 2 {
		t.Fatalf("a respawn in place translated to %d packets", len(respawned))
	}
	if speed, ok := respawned[0].(*packet.UpdateAttributes); !ok || speed.Attributes[0].Name != bridgecontract.JavaMovementAttribute || len(speed.Attributes[0].Modifiers) != 1 {
		t.Fatalf("a respawn in place first translated to %+v", respawned[0])
	}
	if _, ok := respawned[1].(*packet.SetPlayerGameType); !ok {
		t.Fatalf("a respawn in place then translated to %T", respawned[1])
	}
	placed := ts.serve(&java.PlayerPositionAndLook{X: 2.5, Y: 1 + eyeHeight, Z: 0.5, OnGround: true})
	var ready *packet.Respawn
	for _, pk := range placed {
		if respawn, ok := pk.(*packet.Respawn); ok {
			ready = respawn
		}
	}
	if ready == nil || ready.State != packet.RespawnStateReadyToSpawn || ready.Position[0] != 2.5 {
		t.Fatalf("the position after a respawn translated to %+v", placed)
	}
	// The column is still known, so a change inside it reaches the client.
	if changed := ts.serve(&java.BlockChange{X: 1, Y: 0, Z: 0, BlockID: 1}); changed[0].(*packet.UpdateBlock).Position != (protocol.BlockPos{1, 0, 0}) {
		t.Fatalf("block change after a respawn in place = %+v", changed[0])
	}
	// The mob is still there too: spawning it again replaces it.
	again := ts.serve(&java.SpawnMob{EntityID: 9, Type: 90})
	if removed, ok := again[0].(*packet.RemoveActor); !ok || removed.EntityUniqueID != uniqueID(9) || len(again) != 2 {
		t.Fatalf("a repeated spawn translated to %+v", again)
	}

	moved := ts.serve(&java.Respawn{Dimension: -1, LevelType: "default"})
	var change *packet.ChangeDimension
	removals := 0
	for _, pk := range moved {
		switch pk := pk.(type) {
		case *packet.ChangeDimension:
			change = pk
		case *packet.RemoveActor:
			removals++
		}
	}
	if change == nil || change.Dimension != bedrockNether || removals != 1 {
		t.Fatalf("a change of dimension translated to %+v", moved)
	}
	// The old world is gone: its columns no longer take changes, so the next batch is the
	// marker's.
	if err := ts.server.Write(&java.BlockChange{X: 1, Y: 0, Z: 0, BlockID: 1}); err != nil {
		t.Fatal(err)
	}
	if next := ts.serve(&java.ChatMessage{Message: `"marker"`}); len(next) != 1 || next[0].ID() != packet.IDText {
		t.Fatalf("a change in the forgotten world reached the client: %+v", next)
	}
}

// A respawn in place waits for the position that tells the client where it respawned. A
// dimension change before that position ends the wait, so a later teleport is only a
// teleport.
func TestADimensionChangeEndsAPendingRespawn(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 1 + eyeHeight, Z: 0.5, OnGround: true})
	ts.serve(&java.Respawn{Dimension: 0, LevelType: "default"})
	ts.serve(&java.Respawn{Dimension: -1, LevelType: "default"})
	placed := ts.serve(&java.PlayerPositionAndLook{X: 2.5, Y: 1 + eyeHeight, Z: 0.5, OnGround: true})
	for _, pk := range placed {
		if _, ok := pk.(*packet.Respawn); ok {
			t.Fatalf("a teleport after a dimension change translated to a respawn: %+v", placed)
		}
	}
}

// Servers send empty chat lines to space out their messages. Bedrock text cannot be empty,
// so the line reaches the client blank instead of failing to encode and ending the core.
func TestAnEmptyChatLineReachesTheClientBlank(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	for _, raw := range []string{`{"text":""}`, `""`, `{"extra":[],"text":""}`} {
		lines := ts.serve(&java.ChatMessage{Message: raw})
		if len(lines) != 1 {
			t.Fatalf("chat %s translated to %d packets", raw, len(lines))
		}
		if text, ok := lines[0].(*packet.Text); !ok || text.Message != " " {
			t.Fatalf("chat %s translated to %+v", raw, lines[0])
		}
	}
}

// A packet the Bedrock encoder refuses is an error, not a panic.
func TestAnUnencodablePacketIsAnError(t *testing.T) {
	if _, err := encodeBedrock(&packet.Text{TextType: packet.TextTypeRaw}); err == nil {
		t.Fatal("an empty chat message encoded")
	}
}

func TestClientFlightChatLengthAndRadiusChanges(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	// The 1.7.10 client sends no movement while its column is not loaded.
	ts.loadColumn(0, 0)
	ts.serve(&java.PlayerAbilities{AllowFlying: true, CreativeMode: true, FlySpeed: 0.05, WalkSpeed: 0.1})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)

	// Taking off is reported with the abilities the server granted, before the movement.
	input := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, float32(64 + networkOffset), 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	input.InputData.Set(packet.InputFlagStartFlying)
	ts.write(input)
	flying := expect[*java.ClientPlayerAbilities](ts)
	if !flying.Flying || !flying.AllowFlying || !flying.CreativeMode || flying.FlySpeed != 0.05 {
		t.Fatalf("flight start = %+v", flying)
	}
	expect[*java.ClientPlayerPosition](ts)

	// The server counts UTF-16 units: sixty characters outside the basic plane are 120.
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: strings.Repeat("😀", 60)})
	if chat := expect[*java.ClientChatMessage](ts); chat.Message != strings.Repeat("😀", 50) {
		t.Fatalf("chat kept %d characters", utf8.RuneCountInString(chat.Message))
	}

	// A new radius is granted and the published area follows it.
	for len(ts.session.batches) > 0 {
		ts.batch()
	}
	ts.write(&packet.RequestChunkRadius{ChunkRadius: 4})
	granted := ts.batch()
	publisher, ok := granted[1].(*packet.NetworkChunkPublisherUpdate)
	if radius := granted[0].(*packet.ChunkRadiusUpdated); radius.ChunkRadius != 4 || !ok || publisher.Radius != 64 {
		t.Fatalf("radius change translated to %+v", granted)
	}
}

// A freshly started 1.7.10 server gives its first entity, often the joining player, id 0,
// which Bedrock reserves; the player's ids stay valid and an attack on entity 0 reaches it.
func TestEntityZeroGetsValidBedrockIDs(t *testing.T) {
	ts := startTestSession(t)
	joined := ts.serve(&java.JoinGame{EntityID: 0, LevelType: "default"})
	start, ok := joined[0].(*packet.StartGame)
	if !ok || start.EntityRuntimeID == 0 || start.EntityUniqueID == 0 {
		t.Fatalf("StartGame for entity 0 = %+v", joined[0])
	}
	if javaEntityID(start.EntityRuntimeID) != 0 {
		t.Fatalf("runtime id %d does not map back to entity 0", start.EntityRuntimeID)
	}
}

// Abilities name the local player by the unique id StartGame gave it, or the client drops
// them and flight never applies.
func TestAbilitiesTargetTheLocalPlayer(t *testing.T) {
	ts := startTestSession(t)
	joined := ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	start := joined[0].(*packet.StartGame)
	var abilities *packet.UpdateAbilities
	for _, pk := range ts.serve(&java.PlayerAbilities{AllowFlying: true, FlySpeed: 0.05, WalkSpeed: 0.1}) {
		if update, ok := pk.(*packet.UpdateAbilities); ok {
			abilities = update
		}
	}
	if abilities == nil || abilities.AbilityData.EntityUniqueID != start.EntityUniqueID {
		t.Fatalf("abilities = %+v, want unique id %d", abilities, start.EntityUniqueID)
	}
}

// Player-list entries name the player's entity when it is known, and otherwise an id no
// entity can have.
func TestPlayerListEntriesNameTheirEntity(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	listed := func(packets []packet.Packet) protocol.PlayerListEntry {
		t.Helper()
		for _, pk := range packets {
			if list, ok := pk.(*packet.PlayerList); ok && len(list.Entries) == 1 {
				return list.Entries[0]
			}
		}
		t.Fatalf("no player-list entry in %+v", packets)
		return protocol.PlayerListEntry{}
	}
	spawned := listed(ts.serve(&java.SpawnPlayer{EntityID: 9, UUID: "00000000-0000-4000-8000-000000000009", Name: "Alex"}))
	if spawned.EntityUniqueID != uniqueID(9) {
		t.Fatalf("a spawned player's entry names %d, want %d", spawned.EntityUniqueID, uniqueID(9))
	}
	unseen := listed(ts.serve(&java.PlayerListItem{Name: "Notch", Online: true}))
	if unseen.EntityUniqueID <= uniqueID(-1) {
		t.Fatalf("an unseen player's entry names %d, inside the entity ids", unseen.EntityUniqueID)
	}
	// The session logs in as Steve.
	own := listed(ts.serve(&java.PlayerListItem{Name: "Steve", Online: true}))
	if own.EntityUniqueID != uniqueID(7) {
		t.Fatalf("the local player's entry names %d, want %d", own.EntityUniqueID, uniqueID(7))
	}
}

// The local player's own list entry is recognised whatever case the server writes its name in.
func TestOwnPlayerListEntryIgnoresCase(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	for _, pk := range ts.serve(&java.PlayerListItem{Name: "STEVE", Online: true}) {
		if list, ok := pk.(*packet.PlayerList); ok && list.Entries[0].EntityUniqueID != uniqueID(7) {
			t.Fatalf("the local player's entry names %d, want %d", list.Entries[0].EntityUniqueID, uniqueID(7))
		}
	}
}

// A rejected transaction and a keep-alive reach the client as latency probes and are answered
// only when the client echoes them, after the packets before them, as the 1.7.10 client
// answers them from its tick. Anticheats time knockback and teleports by these answers.
func TestProbesAreAnsweredByTheClient(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})

	var probes []*packet.NetworkStackLatency
	collect := func(packets []packet.Packet) {
		for _, pk := range packets {
			if latency, ok := pk.(*packet.NetworkStackLatency); ok {
				probes = append(probes, latency)
			}
		}
	}
	collect(ts.serve(&java.ConfirmTransaction{WindowID: 0, Action: -12, Accepted: false}))
	collect(ts.serve(&java.KeepAlive{Key: 99}))
	if len(probes) != 2 || !probes[0].NeedsResponse || !probes[1].NeedsResponse {
		t.Fatalf("probes = %+v", probes)
	}
	// An accepted transaction needs no answer.
	ts.send(&java.ConfirmTransaction{WindowID: 0, Action: 3, Accepted: true})
	if next := ts.serve(&java.ChatMessage{Message: `"marker"`}); len(next) != 1 || next[0].ID() != packet.IDText {
		t.Fatalf("an accepted transaction reached the client: %+v", next)
	}

	ts.write(&packet.NetworkStackLatency{Timestamp: probes[0].Timestamp * probeTimestampScale})
	if confirm := expect[*java.ClientConfirmTransaction](ts); confirm.WindowID != 0 || confirm.Action != -12 || !confirm.Accepted {
		t.Fatalf("confirmation = %+v", confirm)
	}
	ts.write(&packet.NetworkStackLatency{Timestamp: probes[1].Timestamp * probeTimestampScale})
	if alive := expect[*java.ClientKeepAlive](ts); alive.Key != 99 {
		t.Fatalf("keep-alive = %+v", alive)
	}
}

// The local player's movement speed snapshot reaches the client whole: its base and every
// modifier with exact amounts, the sprint one too, numbered so a repeated snapshot still
// counts, as the 1.7.10 client applies each one.
func TestMovementSpeedSnapshotsReachTheClientWhole(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	speedII := uuid.MustParse("91aeaa56-376b-4498-935b-2f7f68070635")
	sprint := uuid.MustParse("662a6b8d-da3e-4c1c-8813-96ea6097278d")
	properties := &java.EntityProperties{EntityID: 7, Properties: []java.EntityProperty{{
		Key: movementSpeedKey, Value: 0.1, Modifiers: []java.AttributeModifier{
			{UUID: speedII, Amount: 0.4000000059604645, Operation: 2},
			{UUID: sprint, Amount: 0.30000001192092896, Operation: 2},
		},
	}}}
	var sequences []float32
	for range 2 {
		update, ok := ts.serve(properties)[0].(*packet.UpdateAttributes)
		if !ok || len(update.Attributes) != 1 || update.Attributes[0].Name != bridgecontract.JavaMovementAttribute {
			t.Fatalf("properties translated to %+v", update)
		}
		snapshot := update.Attributes[0]
		sequences = append(sequences, snapshot.Value)
		want := map[string]float64{"base": 0.1, speedII.String(): 0.4000000059604645, sprint.String(): 0.30000001192092896}
		if len(snapshot.Modifiers) != len(want) {
			t.Fatalf("modifiers = %+v", snapshot.Modifiers)
		}
		for _, modifier := range snapshot.Modifiers {
			bits, err := strconv.ParseUint(modifier.Name, 16, 64)
			if err != nil || math.Float64frombits(bits) != want[modifier.ID] {
				t.Fatalf("modifier %s carries %q, want %v", modifier.ID, modifier.Name, want[modifier.ID])
			}
		}
	}
	if sequences[0] == sequences[1] {
		t.Fatalf("a repeated snapshot kept its sequence %v", sequences[0])
	}
}

// The local player's potion effects reach the client under the same ids, and a respawn ends
// them as the 1.7.10 client's new player has none.
func TestEffectsReachTheClient(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	effect := func(packets []packet.Packet) *packet.MobEffect {
		t.Helper()
		for _, pk := range packets {
			if effect, ok := pk.(*packet.MobEffect); ok {
				return effect
			}
		}
		t.Fatalf("no effect in %+v", packets)
		return nil
	}
	added := effect(ts.serve(&java.EntityEffect{EntityID: 7, EffectID: 8, Amplifier: 1, Duration: 600}))
	if added.Operation != packet.MobEffectAdd || added.EffectType != 8 || added.Amplifier != 1 || added.Duration != 600 || added.EntityRuntimeID != runtimeID(7) {
		t.Fatalf("jump boost = %+v", added)
	}
	if again := effect(ts.serve(&java.EntityEffect{EntityID: 7, EffectID: 8, Amplifier: 2, Duration: 300})); again.Operation != packet.MobEffectModify {
		t.Fatalf("a renewed effect = %+v", again)
	}
	if removed := effect(ts.serve(&java.RemoveEntityEffect{EntityID: 7, EffectID: 8})); removed.Operation != packet.MobEffectRemove || removed.EffectType != 8 {
		t.Fatalf("removal = %+v", removed)
	}
	ts.serve(&java.EntityEffect{EntityID: 7, EffectID: 15, Duration: 100})
	if ended := effect(ts.serve(&java.Respawn{Dimension: 0, LevelType: "default"})); ended.Operation != packet.MobEffectRemove || ended.EffectType != 15 {
		t.Fatalf("respawn = %+v", ended)
	}
}

// loadColumn serves a column with one stone block at its corner.
func (ts *testSession) loadColumn(x, z int32) {
	ts.t.Helper()
	column := newJavaColumn()
	column.set(0, 0, 0, 1, 0)
	primary, add, data := column.data()
	ts.serve(&java.ChunkData{ChunkX: x, ChunkZ: z, GroundUpContinuous: true, PrimaryBitmask: primary, AddBitmask: add, Compressed: compress(ts.t, data)})
}

// Movement is sent even while the player's column is not loaded: the 1.7.10 client's
// ChunkProviderClient reports every chunk as existing, so EntityClientPlayerMP never skips
// its update.
func TestMovementOutsideLoadedColumns(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	ts.write(&packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, float32(64 + networkOffset), 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)})
	next, err := ts.server.Read()
	if err != nil {
		t.Fatal(err)
	}
	switch next.(type) {
	case *java.ClientPlayer, *java.ClientPlayerPosition, *java.ClientPlayerLook, *java.ClientPlayerPositionLook:
	default:
		t.Fatalf("a tick outside loaded columns sent %+v", next)
	}
}

// The 1.7.10 client's yaw turns without bound, so crossing the Bedrock client's ±180° wrap
// reads as a small turn, not a full circle.
func TestYawTurnsWithoutWrapping(t *testing.T) {
	var look yawUnwrapper
	look.set(170)
	for _, step := range []struct{ raw, want float32 }{{179, 179}, {-179, 181}, {-170, 190}, {170, 170}} {
		if got := look.unwrap(step.raw); got != step.want {
			t.Fatalf("yaw after %v = %v, want %v", step.raw, got, step.want)
		}
	}
	look.set(725)
	if got := look.unwrap(6); got != 726 {
		t.Fatalf("yaw after a teleport to 725 = %v, want 726", got)
	}
}

// The Bedrock client asks to respawn in two packets; the server hears one request.
func TestRespawnIsRequestedOnce(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.UpdateHealth{Health: 0, Food: 20})
	ts.write(&packet.PlayerAction{ActionType: protocol.PlayerActionRespawn})
	ts.write(&packet.Respawn{State: packet.RespawnStateClientReadyToSpawn})
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: "marker"})
	if status := expect[*java.ClientStatus](ts); status.Action != clientStatusRespawn {
		t.Fatalf("status = %+v", status)
	}
	if next, err := ts.server.Read(); err != nil {
		t.Fatal(err)
	} else if chat, ok := next.(*java.ClientChatMessage); !ok || chat.Message != "marker" {
		t.Fatalf("a second respawn request went out: %+v", next)
	}
}

// Swings reach the server as the 1.7.10 client sends them: for an attack, never for a throw,
// and on every tick of mining after that tick's digging.
func TestSwingsFollowTheJavaClient(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.loadColumn(0, 0)
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	next := func() java.Packet {
		t.Helper()
		pk, err := ts.server.Read()
		if err != nil {
			t.Fatal(err)
		}
		return pk
	}
	ts.write(&packet.Animate{ActionType: packet.AnimateActionSwingArm, SwingSource: packet.AnimateSwingSourceThrowItem})
	ts.write(&packet.Animate{ActionType: packet.AnimateActionSwingArm, SwingSource: packet.AnimateSwingSourceAttack})
	if swing, ok := next().(*java.ClientAnimation); !ok || swing.Animation != 1 {
		t.Fatalf("the attack swing = %+v", swing)
	}
	eye := float32(64 + networkOffset)
	mining := func(actions ...protocol.PlayerBlockAction) *packet.PlayerAuthInput {
		pk := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		pk.InputData.Set(packet.InputFlagVerticalCollision)
		if len(actions) > 0 {
			pk.BlockActions = protocol.Option(actions)
			pk.InputData.Set(packet.InputFlagPerformBlockActions)
		}
		return pk
	}
	ts.write(mining(protocol.PlayerBlockAction{Action: protocol.PlayerActionStartBreak, BlockPos: protocol.BlockPos{0, 0, 0}, Face: 1}))
	if dig, ok := next().(*java.ClientPlayerDigging); !ok || dig.Status != digStart {
		t.Fatalf("the mining tick first sent %+v", dig)
	}
	if _, ok := next().(*java.ClientAnimation); !ok {
		t.Fatal("the mining tick sent no swing after its dig")
	}
	next()
	ts.write(mining())
	if _, ok := next().(*java.ClientAnimation); !ok {
		t.Fatal("a tick of continued mining sent no swing")
	}
}

// An explosion destroys its blocks and gives the client the push it adds to the player's
// motion, numbered so each one applies once, with the recent ones repeated.
func TestExplosionsDestroyBlocksAndPushThePlayer(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.loadColumn(0, 0)
	push := func(packets []packet.Packet) protocol.Attribute {
		t.Helper()
		for _, pk := range packets {
			if update, ok := pk.(*packet.UpdateAttributes); ok && update.Attributes[0].Name == bridgecontract.JavaPushAttribute {
				return update.Attributes[0]
			}
		}
		t.Fatalf("no push in %+v", packets)
		return protocol.Attribute{}
	}
	first := ts.serve(&java.Explosion{X: 0.5, Y: 0.5, Z: 0.5, Records: []java.ExplosionRecord{{}}, PlayerMotionX: 0.25, PlayerMotionY: 0.5})
	destroyed := false
	for _, pk := range first {
		if update, ok := pk.(*packet.UpdateBlock); ok && update.Position == (protocol.BlockPos{0, 0, 0}) {
			destroyed = true
		}
	}
	if !destroyed {
		t.Fatalf("the explosion's block was not destroyed: %+v", first)
	}
	event := push(first)
	if event.Value != 1 || len(event.Modifiers) != 3 || event.Modifiers[0].ID != "1:x" || event.Modifiers[0].Amount != 0.25 || event.Modifiers[1].Amount != 0.5 {
		t.Fatalf("first push = %+v", event)
	}
	event = push(ts.serve(&java.Explosion{X: 100, Y: 100, Z: 100, PlayerMotionZ: -1}))
	if event.Value != 2 || len(event.Modifiers) != 6 || event.Modifiers[5].ID != "2:z" || event.Modifiers[5].Amount != -1 {
		t.Fatalf("second push = %+v", event)
	}
}

// After a server teleport, movement the client simulated before it applied the teleport is
// held back, even close to the target, until the client answers the probe sent after it.
func TestMovementFromBeforeATeleportIsHeldBack(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.loadColumn(0, 0)
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	eye := float32(64 + networkOffset)
	input := func(z float32) *packet.PlayerAuthInput {
		return &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, z}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	}
	ts.write(input(0.5))
	expect[*java.ClientPlayerPosition](ts)

	var probe *packet.NetworkStackLatency
	for _, pk := range ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.7, OnGround: true}) {
		if latency, ok := pk.(*packet.NetworkStackLatency); ok {
			probe = latency
		}
	}
	if probe == nil {
		t.Fatal("the teleport sent no probe")
	}
	if reply := expect[*java.ClientPlayerPositionLook](ts); reply.Z != 0.7 {
		t.Fatalf("teleport reply = %+v", reply)
	}
	// A tick from before the teleport, a hair from its target.
	ts.write(input(0.6))
	ts.write(&packet.NetworkStackLatency{Timestamp: probe.Timestamp * probeTimestampScale})
	ts.write(input(0.7))
	next, err := ts.server.Read()
	if err != nil {
		t.Fatal(err)
	}
	if moved, ok := next.(*java.ClientPlayerPosition); !ok || math.Abs(moved.Z-0.7) > 1e-6 {
		t.Fatalf("the first movement after the teleport = %+v, want the teleport position", next)
	}
}

// Letting go of a use early is reported; a use that runs its course, such as a finished meal,
// ends without a packet, as in the 1.7.10 client.
func TestOnlyAnEarlyReleaseIsReported(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: "joined"})
	if chat := expect[*java.ClientChatMessage](ts); chat.Message != "joined" {
		t.Fatalf("chat = %+v", chat)
	}
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.ReleaseItemTransactionData{ActionType: protocol.ReleaseItemActionConsume}})
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.ReleaseItemTransactionData{ActionType: protocol.ReleaseItemActionRelease}})
	next, err := ts.server.Read()
	if err != nil {
		t.Fatal(err)
	}
	if dig, ok := next.(*java.ClientPlayerDigging); !ok || dig.Status != digRelease {
		t.Fatalf("first packet = %+v, want the early release only", next)
	}
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: "marker"})
	if next, err := ts.server.Read(); err != nil {
		t.Fatal(err)
	} else if chat, ok := next.(*java.ClientChatMessage); !ok || chat.Message != "marker" {
		t.Fatalf("a finished use was reported: %+v", next)
	}
}

// A sprint that starts in a tick held back after a teleport still reaches the server with the
// next movement that is sent, as the client reports the start only once.
func TestASprintStartedInAHeldBackTickReachesTheServer(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	eye := float32(64 + networkOffset)
	input := func(flags ...int) *packet.PlayerAuthInput {
		pk := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		for _, flag := range flags {
			pk.InputData.Set(flag)
		}
		return pk
	}
	ts.write(input())
	var probe *packet.NetworkStackLatency
	for _, pk := range ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true}) {
		if latency, ok := pk.(*packet.NetworkStackLatency); ok {
			probe = latency
		}
	}
	ts.write(input(packet.InputFlagStartSprinting))
	ts.write(&packet.NetworkStackLatency{Timestamp: probe.Timestamp * probeTimestampScale})
	ts.write(input())
	if action := expect[*java.ClientEntityAction](ts); action.Action != actionStartSprinting {
		t.Fatalf("action = %+v", action)
	}
}

// Only a rejected transaction for the player's inventory is confirmed: the 1.7.10 client
// answers no window it has not opened, and the translator opens none.
func TestTransactionsForOtherWindowsAreNotConfirmed(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.send(&java.ConfirmTransaction{WindowID: 3, Action: -1, Accepted: false})
	if next := ts.serve(&java.ChatMessage{Message: `"marker"`}); len(next) != 1 || next[0].ID() != packet.IDText {
		t.Fatalf("a transaction for window 3 was probed: %+v", next)
	}
}

// A respawn request from a living player, as the Bedrock client sends after a respawn that
// already happened, is not passed on; a dead player's is.
func TestOnlyADeadPlayerRequestsARespawn(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.UpdateHealth{Health: 20, Food: 20})
	ts.write(&packet.Respawn{State: packet.RespawnStateClientReadyToSpawn})
	ts.serve(&java.UpdateHealth{Health: 0, Food: 20})
	ts.write(&packet.PlayerAction{ActionType: protocol.PlayerActionRespawn})
	ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: "marker"})
	if status := expect[*java.ClientStatus](ts); status.Action != clientStatusRespawn {
		t.Fatalf("status = %+v", status)
	}
	if next, err := ts.server.Read(); err != nil {
		t.Fatal(err)
	} else if chat, ok := next.(*java.ClientChatMessage); !ok || chat.Message != "marker" {
		t.Fatalf("after the dead player's request = %+v", next)
	}
}

// javaAttributesFixture is the shared fixture the client's movement facts decode: the
// attributes the translator sends for a speed snapshot and two explosions, with the values
// they carry as exact bits.
const javaAttributesFixture = "../../../crates/gameplay/fixtures/java_attributes.json"

// The speed snapshot and explosion push encodings, as the translator writes them, in a fixture
// the Rust decoders read back.
func TestJavaAttributeEncodingsMatchTheSharedFixture(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.loadColumn(0, 0)
	speedII := uuid.MustParse("91aeaa56-376b-4498-935b-2f7f68070635")
	base, speed := 0.10000000149011612, 0.123456789012345
	encoded := func(packets []packet.Packet, name string) protocol.Attribute {
		t.Helper()
		for _, pk := range packets {
			if update, ok := pk.(*packet.UpdateAttributes); ok && update.Attributes[0].Name == name {
				return update.Attributes[0]
			}
		}
		t.Fatalf("no %s in %+v", name, packets)
		return protocol.Attribute{}
	}
	snapshot := encoded(ts.serve(&java.EntityProperties{EntityID: 7, Properties: []java.EntityProperty{{
		Key: movementSpeedKey, Value: base, Modifiers: []java.AttributeModifier{{UUID: speedII, Amount: speed, Operation: 2}},
	}}}), bridgecontract.JavaMovementAttribute)
	ts.serve(&java.Explosion{X: 100, Y: 100, Z: 100, PlayerMotionX: 0.25, PlayerMotionY: 0.5, PlayerMotionZ: -0.125})
	pushes := encoded(ts.serve(&java.Explosion{X: 100, Y: 100, Z: 100, PlayerMotionX: -1.5, PlayerMotionY: 0.0625, PlayerMotionZ: 3}), bridgecontract.JavaPushAttribute)
	type modifier struct {
		ID        string  `json:"id"`
		Name      string  `json:"name"`
		Amount    float32 `json:"amount"`
		Operation int32   `json:"operation"`
	}
	attributeJSON := func(attribute protocol.Attribute) map[string]any {
		modifiers := []modifier{}
		for _, m := range attribute.Modifiers {
			modifiers = append(modifiers, modifier{ID: m.ID, Name: m.Name, Amount: m.Amount, Operation: m.Operation})
		}
		return map[string]any{"name": attribute.Name, "current": attribute.Value, "modifiers": modifiers}
	}
	bits := func(value float64) string { return strconv.FormatUint(math.Float64bits(value), 16) }
	fixture, err := json.MarshalIndent(map[string]any{
		"speed":  attributeJSON(snapshot),
		"pushes": attributeJSON(pushes),
		"expected": map[string]any{
			"speed_base":          bits(base),
			"speed_modifier_id":   strings.ReplaceAll(speedII.String(), "-", ""),
			"speed_modifier_bits": bits(speed),
			"pushes": [][3]string{
				{bits(0.25), bits(0.5), bits(-0.125)},
				{bits(-1.5), bits(0.0625), bits(3)},
			},
		},
	}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	checkFixture(t, javaAttributesFixture, append(fixture, '\n'))
}

// A sprint the client keeps through a respawn is reported again for the new player, as the
// 1.7.10 client's new player reports its sprint afresh, though the client sends no new start.
func TestASprintKeptThroughARespawnIsReportedAgain(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	eye := float32(64 + networkOffset)
	input := func(flags ...int) *packet.PlayerAuthInput {
		pk := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		for _, flag := range flags {
			pk.InputData.Set(flag)
		}
		return pk
	}
	ts.write(input(packet.InputFlagStartSprinting))
	if action := expect[*java.ClientEntityAction](ts); action.Action != actionStartSprinting {
		t.Fatalf("action = %+v", action)
	}
	ts.serve(&java.Respawn{Dimension: -1, LevelType: "default"})
	var probe *packet.NetworkStackLatency
	for _, pk := range ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true}) {
		if latency, ok := pk.(*packet.NetworkStackLatency); ok {
			probe = latency
		}
	}
	expect[*java.ClientPlayerPositionLook](ts)
	ts.write(&packet.NetworkStackLatency{Timestamp: probe.Timestamp * probeTimestampScale})
	ts.write(input())
	if action := expect[*java.ClientEntityAction](ts); action.Action != actionStartSprinting {
		t.Fatalf("after the respawn = %+v", action)
	}
}

// A sprint the client ends across a respawn sends no action: the new player never reported
// it, so its stop is not reported either.
func TestASprintEndedAcrossARespawnSendsNothing(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	eye := float32(64 + networkOffset)
	input := func(flags ...int) *packet.PlayerAuthInput {
		pk := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, eye, 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
		for _, flag := range flags {
			pk.InputData.Set(flag)
		}
		return pk
	}
	ts.write(input(packet.InputFlagStartSprinting))
	expect[*java.ClientEntityAction](ts)
	ts.serve(&java.Respawn{Dimension: -1, LevelType: "default"})
	var probe *packet.NetworkStackLatency
	for _, pk := range ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true}) {
		if latency, ok := pk.(*packet.NetworkStackLatency); ok {
			probe = latency
		}
	}
	expect[*java.ClientPlayerPositionLook](ts)
	ts.write(&packet.NetworkStackLatency{Timestamp: probe.Timestamp * probeTimestampScale})
	ts.write(input(packet.InputFlagStopSprinting))
	next, err := ts.server.Read()
	if err != nil {
		t.Fatal(err)
	}
	if action, ok := next.(*java.ClientEntityAction); ok {
		t.Fatalf("an ended sprint sent %+v", action)
	}
}

// A swing the server plays on the local player is reported back once the client plays it, as
// EntityClientPlayerMP.swingItem does; a held slot outside the hotbar is ignored, as
// handleHeldItemChange ignores it. Anti-bot verifications check both.
func TestServerSwingsAreEchoedAndBadSlotsIgnored(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
	ts.serve(&java.PlayerPositionAndLook{X: 0.5, Y: 64 + eyeHeight, Z: 0.5, OnGround: true})
	expect[*java.ClientPlayerPositionLook](ts)
	probeIn := func(served []packet.Packet) *packet.NetworkStackLatency {
		for _, pk := range served {
			if latency, ok := pk.(*packet.NetworkStackLatency); ok {
				return latency
			}
		}
		return nil
	}
	// untilChat writes a chat marker and returns what the translator sends up to it.
	untilChat := func(marker string) []java.Packet {
		ts.write(&packet.Text{TextType: packet.TextTypeChat, Message: marker})
		var sent []java.Packet
		for {
			pk, err := ts.server.Read()
			if err != nil {
				t.Fatalf("read from the translator: %v", err)
			}
			if chat, ok := pk.(*java.ClientChatMessage); ok && chat.Message == marker {
				return sent
			}
			sent = append(sent, pk)
		}
	}

	// servedUntilMarker writes pks and a chat marker and returns what the client receives up
	// to the marker, for packets that may translate to nothing.
	servedUntilMarker := func(pks ...java.Packet) []packet.Packet {
		for _, pk := range append(pks, &java.ChatMessage{Message: `"marker"`}) {
			if err := ts.server.Write(pk); err != nil {
				t.Fatal(err)
			}
		}
		var served []packet.Packet
		for {
			batch := ts.batch()
			served = append(served, batch...)
			for _, pk := range batch {
				if _, ok := pk.(*packet.Text); ok {
					return served
				}
			}
		}
	}

	// Another entity's swing and the player's own hurt animation are not echoed.
	if probeIn(servedUntilMarker(
		&java.Animation{EntityID: 8, Animation: animationSwingArm},
		&java.Animation{EntityID: 7, Animation: animationHurt},
	)) != nil {
		t.Fatal("an animation other than the player's swing sent a probe")
	}
	probe := probeIn(ts.serve(&java.Animation{EntityID: 7, Animation: animationSwingArm}))
	if probe == nil {
		t.Fatal("the swing sent no probe")
	}
	// Nothing is echoed before the client has played the swing.
	for _, pk := range untilChat("before") {
		if _, ok := pk.(*java.ClientAnimation); ok {
			t.Fatal("the swing was echoed before the client answered its probe")
		}
	}
	ts.write(&packet.NetworkStackLatency{Timestamp: probe.Timestamp * probeTimestampScale})
	if swing := expect[*java.ClientAnimation](ts); swing.EntityID != 7 || swing.Animation != 1 {
		t.Fatalf("swing echo = %+v", swing)
	}

	input := &packet.PlayerAuthInput{Position: mgl32.Vec3{0.5, float32(64 + networkOffset), 0.5}, InputData: protocol.NewInputFlags(packet.InputFlagCount)}
	for _, pk := range servedUntilMarker(&java.HeldItemChange{Slot: 9}) {
		if _, ok := pk.(*packet.PlayerHotBar); ok {
			t.Fatal("a slot outside the hotbar reached the client")
		}
	}
	ts.write(input)
	for _, pk := range untilChat("after") {
		if held, ok := pk.(*java.ClientHeldItemChange); ok {
			t.Fatalf("a slot outside the hotbar was reported: %+v", held)
		}
	}
	ts.serve(&java.HeldItemChange{Slot: 4})
	ts.write(input)
	if held := expect[*java.ClientHeldItemChange](ts); held.Slot != 4 {
		t.Fatalf("held slot = %+v", held)
	}
}
