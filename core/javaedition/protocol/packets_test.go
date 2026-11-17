package protocol

import (
	"bytes"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/google/uuid"
)

// samplePackets returns at least one fully populated instance of every packet, plus the
// variants whose wire layout depends on a field.
func samplePackets(t testing.TB) []Packet {
	t.Helper()
	sword := ItemStack{ID: 276, Count: 1, Damage: 17, NBT: mustNBT(t, map[string]any{
		"ench": []any{map[string]any{"id": int16(16), "lvl": int16(4)}},
	})}
	stone := ItemStack{ID: 1, Count: 64, Damage: 2}
	metadata := Metadata{
		{Index: 0, Type: MetadataByte, Value: int8(0x20)},
		{Index: 1, Type: MetadataShort, Value: int16(300)},
		{Index: 12, Type: MetadataInt, Value: int32(-1)},
		{Index: 6, Type: MetadataFloat, Value: float32(19.5)},
		{Index: 10, Type: MetadataString, Value: "Grumm"},
		{Index: 2, Type: MetadataItem, Value: sword},
		{Index: 17, Type: MetadataPosition, Value: Position{X: -5, Y: 70, Z: 12}},
	}
	lines := [4]string{"first", "second", "", "fourth line 15c"}
	return []Packet{
		&Handshake{ProtocolVersion: Version, ServerAddress: "play.example.org.", ServerPort: 25565, NextState: StateLogin},
		&StatusRequest{},
		&StatusPing{Time: 1234567890123},
		&StatusResponse{JSON: `{"description":"A Minecraft Server"}`},
		&StatusPong{Time: -42},
		&LoginStart{Username: "Notch"},
		&EncryptionResponse{SharedSecret: []byte{1, 2, 3}, VerifyToken: []byte{4, 5}},
		&LoginDisconnect{Reason: `{"text":"Outdated client!"}`},
		&EncryptionRequest{ServerID: "", PublicKey: []byte{0x30, 0x81, 0x9f}, VerifyToken: []byte{9, 8, 7, 6}},
		&LoginSuccess{UUID: "069a79f4-44e9-4726-a5be-fca90e38aaf5", Username: "Notch"},

		&KeepAlive{Key: -559038737},
		&JoinGame{EntityID: 4711, GameMode: 1, Hardcore: true, Dimension: -1, Difficulty: 3, MaxPlayers: 200, LevelType: "largeBiomes"},
		&ChatMessage{Message: `{"text":"héllo"}`},
		&TimeUpdate{WorldAge: 123456789012, TimeOfDay: -6000},
		&EntityEquipment{EntityID: 12, Slot: 4, Item: sword},
		&EntityEquipment{EntityID: 12, Slot: 0, Item: EmptyItemStack()},
		&SpawnPosition{X: -100, Y: 64, Z: 250},
		&UpdateHealth{Health: 19.5, Food: 17, Saturation: 2.25},
		&Respawn{Dimension: -1, Difficulty: 2, GameMode: 0, LevelType: "default"},
		&PlayerPositionAndLook{X: 1.5, Y: 65.62, Z: -3.25, Yaw: 90, Pitch: -45, OnGround: true},
		&HeldItemChange{Slot: 8},
		&UseBed{EntityID: 9, X: -30, Y: 64, Z: 77},
		&Animation{EntityID: 300, Animation: 1},
		&SpawnPlayer{
			EntityID: 2097152, UUID: "069a79f4-44e9-4726-a5be-fca90e38aaf5", Name: "Notch",
			Properties: []ProfileProperty{{Name: "textures", Value: "dmFsdWU=", Signature: "c2ln"}},
			X:          3200, Y: 2048, Z: -640, Yaw: -128, Pitch: 12, CurrentItem: 276, Metadata: metadata,
		},
		&CollectItem{CollectedEntityID: 55, CollectorEntityID: 4711},
		&SpawnObject{EntityID: 77, Type: 60, X: 1, Y: 2, Z: 3, Pitch: 4, Yaw: 5, Data: 4711, VelocityX: -8000, VelocityY: 100, VelocityZ: 8000},
		&SpawnObject{EntityID: 78, Type: 2, X: 1, Y: 2, Z: 3, Pitch: 4, Yaw: 5, Data: 0},
		&SpawnMob{EntityID: 99, Type: 200, X: -32, Y: 2080, Z: 64, Yaw: 127, Pitch: -10, HeadYaw: 64, VelocityX: 1, VelocityY: -2, VelocityZ: 3, Metadata: metadata},
		&SpawnPainting{EntityID: 5, Title: "SkullAndRoses", X: 10, Y: 70, Z: -10, Facing: 3},
		&SpawnExperienceOrb{EntityID: 6, X: 320, Y: 2240, Z: -320, Count: 11},
		&EntityVelocity{EntityID: 7, VelocityX: -8000, VelocityY: 3136, VelocityZ: 31200},
		&DestroyEntities{EntityIDs: []int32{1, -2, 300000}},
		&Entity{EntityID: 8},
		&EntityRelativeMove{EntityID: 8, DX: -128, DY: 0, DZ: 127},
		&EntityLook{EntityID: 8, Yaw: -64, Pitch: 32},
		&EntityLookAndRelativeMove{EntityID: 8, DX: 1, DY: -2, DZ: 3, Yaw: -4, Pitch: 5},
		&EntityTeleport{EntityID: 8, X: 32000, Y: 2048, Z: -32000, Yaw: 100, Pitch: -100},
		&EntityHeadLook{EntityID: 8, HeadYaw: -1},
		&EntityStatus{EntityID: 8, Status: 2},
		&AttachEntity{EntityID: 8, VehicleID: -1, Leash: 1},
		&EntityMetadata{EntityID: 8, Metadata: metadata},
		&EntityEffect{EntityID: 8, EffectID: 5, Amplifier: 1, Duration: 32767},
		&RemoveEntityEffect{EntityID: 8, EffectID: 5},
		&SetExperience{Bar: 0.75, Level: 30, TotalExperience: 1395},
		&EntityProperties{EntityID: 8, Properties: []EntityProperty{
			{Key: "generic.movementSpeed", Value: 0.1, Modifiers: []AttributeModifier{
				{UUID: uuid.MustParse("662a6b8d-da3e-4c1c-8813-96ea6097278d"), Amount: 0.3, Operation: 2},
			}},
			{Key: "generic.maxHealth", Value: 20},
		}},
		&ChunkData{ChunkX: -3, ChunkZ: 7, GroundUpContinuous: true, PrimaryBitmask: 0x8001, AddBitmask: 0x0001, Compressed: []byte{0x78, 0x9c, 1, 2, 3}},
		&ChunkData{ChunkX: 3, ChunkZ: -7, GroundUpContinuous: false, PrimaryBitmask: 0x0010, Compressed: []byte{0x78, 0x9c, 9}},
		&MultiBlockChange{ChunkX: 2, ChunkZ: -2, RecordCount: 2, Data: []byte{0x37, 0xc8, 0x4d, 0x29, 0x00, 0x05, 0x00, 0x10}},
		&BlockChange{X: -1, Y: 255, Z: 1, BlockID: 4095, Metadata: 15},
		&BlockAction{X: 5, Y: 64, Z: -5, Data1: 1, Data2: 2, BlockID: 54},
		&BlockBreakAnimation{EntityID: 300, X: 1, Y: 2, Z: 3, Stage: 9},
		&MapChunkBulk{SkyLight: true, Compressed: []byte{0x78, 0x9c, 5, 6, 7, 8}, Columns: []ChunkColumn{
			{ChunkX: 0, ChunkZ: 0, PrimaryBitmask: 0x0003, AddBitmask: 0x0001},
			{ChunkX: -1, ChunkZ: 4, PrimaryBitmask: 0xffff},
		}},
		&Explosion{X: -1.5, Y: 64.5, Z: 8.25, Radius: 4, Records: []ExplosionRecord{{DX: 1, DY: -1, DZ: 0}, {DX: -128, DY: 127, DZ: 3}}, PlayerMotionX: 0.5, PlayerMotionY: 1.5, PlayerMotionZ: -0.5},
		&Effect{EffectID: 2001, X: 10, Y: 200, Z: -10, Data: 35, Global: true},
		&SoundEffect{Name: "random.explode", X: 80, Y: 512, Z: -80, Volume: 4, Pitch: 63},
		&Particle{Name: "hugeexplosion", X: 1.5, Y: 2.5, Z: 3.5, OffsetX: 0.1, OffsetY: 0.2, OffsetZ: 0.3, Speed: 1, Count: 20},
		&ChangeGameState{Reason: 3, Value: 1},
		&SpawnGlobalEntity{EntityID: 400, Type: 1, X: 16, Y: 2048, Z: -16},
		&OpenWindow{WindowID: 3, Type: 0, Title: "container.chest", Slots: 27, UseProvidedTitle: false},
		&OpenWindow{WindowID: 4, Type: 11, Title: "Horse", Slots: 2, UseProvidedTitle: true, EntityID: 1234},
		&CloseWindow{WindowID: 3},
		&SetSlot{WindowID: -1, Slot: -1, Item: sword},
		&WindowItems{WindowID: 0, Items: []ItemStack{EmptyItemStack(), stone, sword}},
		&WindowProperty{WindowID: 3, Property: 1, Value: 200},
		&ConfirmTransaction{WindowID: 3, Action: -5, Accepted: true},
		&UpdateSign{X: 1, Y: 64, Z: -1, Lines: lines},
		&Maps{MapID: 7, Data: []byte{0, 3, 4, 5, 6}},
		&UpdateBlockEntity{X: 1, Y: 64, Z: -1, Action: 1, NBT: mustNBT(t, map[string]any{"EntityId": "Zombie", "Delay": int16(20)})},
		&UpdateBlockEntity{X: 1, Y: 64, Z: -1, Action: 5},
		&SignEditorOpen{X: 1, Y: 64, Z: -1},
		&Statistics{Entries: []Statistic{{Name: "stat.walkOneCm", Value: 123456}, {Name: "achievement.openInventory", Value: 1}}},
		&PlayerListItem{Name: "Notch", Online: true, Ping: 42},
		&PlayerAbilities{Invulnerable: true, AllowFlying: true, FlySpeed: 0.05, WalkSpeed: 0.1},
		&PlayerAbilities{Flying: true, CreativeMode: true, FlySpeed: 0.1, WalkSpeed: 0.2},
		&TabComplete{Matches: []string{"/gamemode", "/gamerule"}},
		&ScoreboardObjective{Name: "kills", DisplayName: "Kills", Mode: 2},
		&UpdateScore{ItemName: "Notch", Action: 0, Objective: "kills", Value: -3},
		&UpdateScore{ItemName: "Notch", Action: 1},
		&DisplayScoreboard{Position: 1, Objective: "kills"},
		&Teams{Name: "red", Mode: TeamCreate, DisplayName: "Red Team", Prefix: "§c", Suffix: "§r", FriendlyFire: 3, Players: []string{"Notch", "jeb_"}},
		&Teams{Name: "red", Mode: TeamRemove},
		&Teams{Name: "red", Mode: TeamUpdate, DisplayName: "Crimson", Prefix: "§4", Suffix: "", FriendlyFire: 1},
		&Teams{Name: "red", Mode: TeamAddPlayers, Players: []string{"Dinnerbone"}},
		&Teams{Name: "red", Mode: TeamRemovePlayers, Players: []string{"Dinnerbone", "Grumm"}},
		&PluginMessage{Channel: "MC|Brand", Data: []byte("vanilla")},
		&Disconnect{Reason: `{"translate":"disconnect.spam"}`},

		&ClientKeepAlive{Key: -559038737},
		&ClientChatMessage{Message: "/me waves"},
		&ClientUseEntity{EntityID: 8, Action: 1},
		&ClientPlayer{OnGround: true},
		&ClientPlayerPosition{X: 1.5, Y: 64, Stance: 65.62, Z: -3.25, OnGround: true},
		&ClientPlayerLook{Yaw: 370.5, Pitch: -89.9, OnGround: false},
		&ClientPlayerPositionLook{X: 1.5, Y: 64, Stance: 65.62, Z: -3.25, Yaw: 90, Pitch: 45, OnGround: true},
		&ClientPlayerDigging{Status: 2, X: -1, Y: 255, Z: 1, Face: 5},
		&ClientBlockPlacement{X: -1, Y: 255, Z: -1, Face: 255, HeldItem: sword, CursorX: 8, CursorY: 16, CursorZ: 0},
		&ClientHeldItemChange{Slot: 8},
		&ClientAnimation{EntityID: 4711, Animation: 1},
		&ClientEntityAction{EntityID: 4711, Action: 6, JumpBoost: 100},
		&ClientSteerVehicle{Strafe: -0.98, Forward: 0.98, Jump: true, Sneak: false},
		&ClientCloseWindow{WindowID: 3},
		&ClientClickWindow{WindowID: 3, Slot: -999, Button: 1, Action: 77, Mode: 4, Item: stone},
		&ClientConfirmTransaction{WindowID: 3, Action: -5, Accepted: true},
		&ClientCreativeInventoryAction{Slot: 36, Item: sword},
		&ClientEnchantItem{WindowID: 3, Enchantment: 2},
		&ClientUpdateSign{X: 1, Y: 64, Z: -1, Lines: lines},
		&ClientPlayerAbilities{Flying: true, AllowFlying: true, FlySpeed: 0.05, WalkSpeed: 0.1},
		&ClientTabComplete{Text: "/game"},
		&ClientSettings{Locale: "en_US", ViewDistance: 12, ChatVisibility: 1, ChatColors: true, Difficulty: 2, ShowCape: true},
		&ClientStatus{Action: 2},
		&ClientPluginMessage{Channel: "MC|Brand", Data: []byte("vanilla")},
	}
}

func TestPacketsRoundTrip(t *testing.T) {
	for _, original := range samplePackets(t) {
		name := fmt.Sprintf("%T", original)
		payload, err := Encode(original)
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		decoded, err := Decode(original.State(), original.Direction(), payload)
		if err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		if !reflect.DeepEqual(decoded, original) {
			t.Errorf("%s read back as\n%+v, want\n%+v", name, decoded, original)
		}
		again, err := Encode(decoded)
		if err != nil || !bytes.Equal(again, payload) {
			t.Errorf("%s: second encoding differs (err %v)", name, err)
		}
	}
}

// TestPacketsRejectTruncationAndTrailingBytes cuts every sample at every length. A decoder
// that accepted a strict prefix would leave bytes unread on the full frame, so each prefix
// must fail; one extra byte must fail as trailing data.
func TestPacketsRejectTruncationAndTrailingBytes(t *testing.T) {
	for _, original := range samplePackets(t) {
		name := fmt.Sprintf("%T", original)
		payload, err := Encode(original)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		for cut := 1; cut < len(payload); cut++ {
			if _, err := Decode(original.State(), original.Direction(), payload[:cut]); err == nil {
				t.Errorf("%s: %d of %d bytes decoded without error", name, cut, len(payload))
				break
			}
		}
		_, err = Decode(original.State(), original.Direction(), append(append([]byte(nil), payload...), 0))
		if !errors.Is(err, ErrTrailingData) {
			t.Errorf("%s with one extra byte: err = %v, want ErrTrailingData", name, err)
		}
	}
}

// TestEveryPacketHasASample fails when a packet is registered without round-trip coverage.
func TestEveryPacketHasASample(t *testing.T) {
	sampled := map[reflect.Type]bool{}
	for _, p := range samplePackets(t) {
		sampled[reflect.TypeOf(p)] = true
	}
	for _, p := range Packets() {
		if !sampled[reflect.TypeOf(p)] {
			t.Errorf("%T has no sample in samplePackets", p)
		}
	}
}

// TestRegistryCoversEveryID checks the registry against the id ranges 1.7.10 assigns: ids are
// dense from zero in each state and direction.
func TestRegistryCoversEveryID(t *testing.T) {
	ranges := []struct {
		state State
		dir   Direction
		count int32
	}{
		{StateHandshake, Serverbound, 1},
		{StateHandshake, Clientbound, 0},
		{StateStatus, Serverbound, 2},
		{StateStatus, Clientbound, 2},
		{StateLogin, Serverbound, 2},
		{StateLogin, Clientbound, 3},
		{StatePlay, Serverbound, 24},
		{StatePlay, Clientbound, 65},
	}
	total := 0
	for _, r := range ranges {
		total += int(r.count)
		for id := int32(0); id < r.count; id++ {
			p, ok := New(r.state, r.dir, id)
			if !ok {
				t.Errorf("%s %s packet %#02x is not registered", r.state, r.dir, id)
				continue
			}
			if p.ID() != id || p.State() != r.state || p.Direction() != r.dir {
				t.Errorf("%s %s packet %#02x constructs %T reporting %s %s %#02x", r.state, r.dir, id, p, p.State(), p.Direction(), p.ID())
			}
		}
		for _, id := range []int32{-1, r.count} {
			if p, ok := New(r.state, r.dir, id); ok {
				t.Errorf("%s %s packet %#02x unexpectedly constructs %T", r.state, r.dir, id, p)
			}
		}
	}
	if len(Packets()) != total || len(registry) != total {
		t.Errorf("%d packets listed and %d indexed, want %d of each", len(Packets()), len(registry), total)
	}
}

func TestDecodeUnknownPacket(t *testing.T) {
	p, err := Decode(StatePlay, Clientbound, []byte{0x7e, 0xca, 0xfe})
	if err != nil {
		t.Fatal(err)
	}
	want := &Unknown{PacketID: 0x7e, PacketState: StatePlay, PacketDirection: Clientbound, Data: []byte{0xca, 0xfe}}
	if !reflect.DeepEqual(p, want) {
		t.Fatalf("decoded %+v, want %+v", p, want)
	}
	payload, err := Encode(p)
	if err != nil || !bytes.Equal(payload, []byte{0x7e, 0xca, 0xfe}) {
		t.Fatalf("unknown packet encoded as % x, err %v", payload, err)
	}
}

func TestPacketVectors(t *testing.T) {
	vectors := []struct {
		packet Packet
		wire   []byte
	}{
		{
			&PlayerPositionAndLook{X: 1, Y: 65.5, Z: -2, Yaw: 90, Pitch: -45, OnGround: true},
			[]byte{
				0x08,
				0x3f, 0xf0, 0, 0, 0, 0, 0, 0,
				0x40, 0x50, 0x60, 0, 0, 0, 0, 0,
				0xc0, 0x00, 0, 0, 0, 0, 0, 0,
				0x42, 0xb4, 0, 0,
				0xc2, 0x34, 0, 0,
				0x01,
			},
		},
		{
			&ClientPlayerPositionLook{X: 1, Y: 64, Stance: 65.5, Z: -2, Yaw: 90, Pitch: -45, OnGround: false},
			[]byte{
				0x06,
				0x3f, 0xf0, 0, 0, 0, 0, 0, 0,
				0x40, 0x50, 0x00, 0, 0, 0, 0, 0,
				0x40, 0x50, 0x60, 0, 0, 0, 0, 0,
				0xc0, 0x00, 0, 0, 0, 0, 0, 0,
				0x42, 0xb4, 0, 0,
				0xc2, 0x34, 0, 0,
				0x00,
			},
		},
		{
			&Handshake{ProtocolVersion: 5, ServerAddress: "mc.io", ServerPort: 25565, NextState: StateLogin},
			[]byte{0x00, 0x05, 0x05, 'm', 'c', '.', 'i', 'o', 0x63, 0xdd, 0x02},
		},
		{
			&JoinGame{EntityID: 1, GameMode: 1, Hardcore: true, Dimension: -1, Difficulty: 2, MaxPlayers: 20, LevelType: "flat"},
			[]byte{0x01, 0, 0, 0, 1, 0x09, 0xff, 0x02, 0x14, 0x04, 'f', 'l', 'a', 't'},
		},
		{
			&SetSlot{WindowID: 0, Slot: 36, Item: ItemStack{ID: 276, Count: 1, Damage: 5}},
			[]byte{0x2f, 0x00, 0x00, 0x24, 0x01, 0x14, 0x01, 0x00, 0x05, 0xff, 0xff},
		},
		{
			&DestroyEntities{EntityIDs: []int32{1, 2}},
			[]byte{0x13, 0x02, 0, 0, 0, 1, 0, 0, 0, 2},
		},
		{
			&UpdateScore{ItemName: "a", Action: 1},
			[]byte{0x3c, 0x01, 'a', 0x01},
		},
		{
			&Teams{Name: "t", Mode: TeamRemovePlayers, Players: []string{"p"}},
			[]byte{0x3e, 0x01, 't', 0x04, 0x00, 0x01, 0x01, 'p'},
		},
	}
	for _, vector := range vectors {
		payload, err := Encode(vector.packet)
		if err != nil {
			t.Errorf("%T: %v", vector.packet, err)
			continue
		}
		if !bytes.Equal(payload, vector.wire) {
			t.Errorf("%T encoded as\n% x, want\n% x", vector.packet, payload, vector.wire)
		}
		decoded, err := Decode(vector.packet.State(), vector.packet.Direction(), vector.wire)
		if err != nil || !reflect.DeepEqual(decoded, vector.packet) {
			t.Errorf("%T decoded as %+v, err %v", vector.packet, decoded, err)
		}
	}
}

func TestStringFieldLimitsAreEnforced(t *testing.T) {
	cases := []struct {
		name   string
		packet Packet
		ok     bool
	}{
		{"chat at 100", &ClientChatMessage{Message: strings.Repeat("a", 100)}, true},
		{"chat at 101", &ClientChatMessage{Message: strings.Repeat("a", 101)}, false},
		{"username at 16", &LoginStart{Username: strings.Repeat("a", 16)}, true},
		{"username at 17", &LoginStart{Username: strings.Repeat("a", 17)}, false},
		{"server id at 21", &EncryptionRequest{ServerID: strings.Repeat("a", 21)}, false},
		{"sign line at 16", &UpdateSign{Lines: [4]string{strings.Repeat("a", 16)}}, false},
		{"painting title at 14", &SpawnPainting{Title: strings.Repeat("a", 14)}, false},
		{"window title at 33", &OpenWindow{Title: strings.Repeat("a", 33)}, false},
		{"team player at 41", &Teams{Name: "t", Mode: TeamAddPlayers, Players: []string{strings.Repeat("a", 41)}}, false},
		{"plugin channel at 21", &PluginMessage{Channel: strings.Repeat("a", 21)}, false},
		{"locale at 8", &ClientSettings{Locale: strings.Repeat("a", 8)}, false},
		{"hostname at 256", &Handshake{ServerAddress: strings.Repeat("a", 256)}, false},
		{"sound name at 257", &SoundEffect{Name: strings.Repeat("a", 257)}, false},
		{"attribute key at 65", &EntityProperties{Properties: []EntityProperty{{Key: strings.Repeat("a", 65)}}}, false},
	}
	for _, tc := range cases {
		payload, err := Encode(tc.packet)
		if err != nil {
			t.Fatalf("%s: %v", tc.name, err)
		}
		_, err = Decode(tc.packet.State(), tc.packet.Direction(), payload)
		if (err == nil) != tc.ok {
			t.Errorf("%s: err = %v, want ok=%v", tc.name, err, tc.ok)
		}
	}
}

func TestLoginSuccessRejectsUnparseableUUID(t *testing.T) {
	for id, ok := range map[string]bool{
		"069a79f4-44e9-4726-a5be-fca90e38aaf5": true,
		"1-2-3-4-5":                            true,
		"069a79f444e94726a5befca90e38aaf5":     false,
		"":                                     false,
		"g69a79f4-44e9-4726-a5be-fca90e38aaf5": false,
	} {
		payload, err := Encode(&LoginSuccess{UUID: id, Username: "Notch"})
		if err != nil {
			t.Fatal(err)
		}
		_, err = Decode(StateLogin, Clientbound, payload)
		if (err == nil) != ok {
			t.Errorf("UUID %q: err = %v, want ok=%v", id, err, ok)
		}
	}
}

func TestNegativeCountsFollowTheClient(t *testing.T) {
	cases := []struct {
		name string
		wire []byte
		ok   bool
	}{
		{"DestroyEntities", []byte{0x13, 0xff}, false},
		{"WindowItems", []byte{0x30, 0x00, 0xff, 0xff}, false},
		{"TabComplete", append([]byte{0x3a}, AppendVarInt(nil, -1)...), false},
		{"Explosion", append(append([]byte{0x27}, make([]byte, 16)...), 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0), false},
		{"Statistics", append([]byte{0x37}, AppendVarInt(nil, -1)...), true},
		{"EntityProperties", []byte{0x20, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff}, true},
		{"Teams players", []byte{0x3e, 0x01, 't', 0x03, 0xff, 0xff}, true},
		{"MultiBlockChange data", []byte{0x22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff}, true},
	}
	for _, tc := range cases {
		_, err := Decode(StatePlay, Clientbound, tc.wire)
		if (err == nil) != tc.ok {
			t.Errorf("%s with a negative count: err = %v, want ok=%v", tc.name, err, tc.ok)
		}
	}
}

func TestHugeCountsDoNotAllocate(t *testing.T) {
	wire := append([]byte{0x3a}, AppendVarInt(nil, 1<<30)...)
	if _, err := Decode(StatePlay, Clientbound, wire); err == nil {
		t.Fatal("a count the frame cannot back was accepted")
	}
}

func TestEncodeReportsUnencodableFields(t *testing.T) {
	cases := []Packet{
		&DestroyEntities{EntityIDs: make([]int32, 128)},
		&ChatMessage{Message: strings.Repeat("a", MaxStringChars+1)},
		&ClientPluginMessage{Channel: "c", Data: make([]byte, 32767)},
		&PluginMessage{Channel: "c", Data: make([]byte, 65536)},
		&EntityMetadata{Metadata: Metadata{{Index: 0, Type: MetadataByte, Value: "wrong"}}},
	}
	for _, p := range cases {
		if _, err := Encode(p); err == nil {
			t.Errorf("%T with an oversized or mistyped field was encoded", p)
		}
	}
}

func TestClientPluginMessageReadsNoDataForBoundaryLengths(t *testing.T) {
	for _, length := range [][]byte{{0x00, 0x00}, {0xff, 0xff}, {0x7f, 0xff}} {
		wire := append([]byte{0x17, 0x01, 'c'}, length...)
		p, err := Decode(StatePlay, Serverbound, wire)
		if err != nil {
			t.Fatalf("length % x: %v", length, err)
		}
		if data := p.(*ClientPluginMessage).Data; data != nil {
			t.Errorf("length % x read %d data bytes", length, len(data))
		}
	}
}

func TestBlockActionMasksBlockID(t *testing.T) {
	payload, err := Encode(&BlockAction{BlockID: 0x1036})
	if err != nil {
		t.Fatal(err)
	}
	p, err := Decode(StatePlay, Clientbound, payload)
	if err != nil {
		t.Fatal(err)
	}
	if id := p.(*BlockAction).BlockID; id != 0x036 {
		t.Fatalf("block id %#x, want the low 12 bits", id)
	}
}
