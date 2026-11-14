package translate

import (
	"bytes"
	"compress/zlib"
	"context"
	"encoding/binary"
	"encoding/json"
	"math"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/javatest"
	"github.com/hashimthearab/rust-mcbe/core/javaedition/legacy"
	java "github.com/hashimthearab/rust-mcbe/core/javaedition/protocol"
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
	server, err := javatest.Start(javatest.Config{})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(server.Close)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	session, err := Dial(ctx, server.Addr(), Options{Username: "Steve"})
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
	if err := ts.session.WritePacketRaw(encodeBedrock(pk)); err != nil {
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
		data := encodeBedrock(registry)
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
	ts := startTestSession(t)
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
	if start.EntityRuntimeID != 7 || !start.UseBlockNetworkIDHashes || start.Dimension != bedrockOverworld {
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
	serve(&java.SpawnPlayer{EntityID: 9, UUID: "00000000-0000-4000-8000-000000000009", Name: "Alex", X: 8 * 32, Y: 1 * 32, Z: 4 * 32, Yaw: 64, CurrentItem: 0,
		Metadata: java.Metadata{{Index: 0, Type: java.MetadataByte, Value: int8(flagSneaking)}}})
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
		data := encodeBedrock(pk)
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
	// Sprinting is announced before the movement it applies to.
	ts.write(input(mgl32.Vec3{1.5, eye, 0.5}, 30, packet.InputFlagVerticalCollision, packet.InputFlagSprinting))
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
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.UseItemOnEntityTransactionData{TargetEntityRuntimeID: 9, ActionType: protocol.UseItemOnEntityActionAttack}})
	if attack := expect[*java.ClientUseEntity](ts); attack.EntityID != 9 || attack.Action != useEntityAttack {
		t.Fatalf("attack = %+v", attack)
	}
	ts.write(&packet.InventoryTransaction{TransactionData: &protocol.UseItemTransactionData{
		ActionType: protocol.UseItemActionClickBlock, BlockPosition: protocol.BlockPos{3, 63, -2}, BlockFace: 1, ClickedPosition: mgl32.Vec3{0.5, 1, 0.25},
	}})
	if place := expect[*java.ClientBlockPlacement](ts); place.X != 3 || place.Y != 63 || place.Z != -2 || place.Face != 1 || place.CursorX != 8 || place.CursorY != 16 || place.CursorZ != 4 {
		t.Fatalf("placement = %+v", place)
	}
	ts.write(&packet.MobEquipment{HotBarSlot: 4, WindowID: protocol.WindowIDInventory})
	if held := expect[*java.ClientHeldItemChange](ts); held.Slot != 4 {
		t.Fatalf("held = %+v", held)
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
	if dig := expect[*java.ClientPlayerDigging](ts); dig.Status != digStart || dig.X != 1 || dig.Y != 63 || dig.Face != 1 {
		t.Fatalf("dig start = %+v", dig)
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
	if _, ok := respawned[0].(*packet.SetPlayerGameType); !ok || len(respawned) != 1 {
		t.Fatalf("a respawn in place translated to %d packets, first %T", len(respawned), respawned[0])
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
	if removed, ok := again[0].(*packet.RemoveActor); !ok || removed.EntityUniqueID != 9 || len(again) != 2 {
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

func TestClientFlightChatLengthAndRadiusChanges(t *testing.T) {
	ts := startTestSession(t)
	ts.serve(&java.JoinGame{EntityID: 7, LevelType: "default"})
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
