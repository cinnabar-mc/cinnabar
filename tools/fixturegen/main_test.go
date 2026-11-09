package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type testManifestEntry struct {
	Name          string `json:"name"`
	File          string `json:"file"`
	ID            uint32 `json:"id"`
	ByteLength    int    `json:"byte_length"`
	SHA256        string `json:"sha256"`
	WireAuthority string `json:"wire_authority,omitempty"`
	WireCommit    string `json:"wire_commit,omitempty"`
}

func TestInteractionFixturesCrossDecodeWithPinnedGophertunnel(t *testing.T) {
	out := t.TempDir()
	if err := generate(out); err != nil {
		t.Fatalf("generate corpus: %v", err)
	}

	filled := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_click_block.bin"))
	assertBlockUseTransaction(t, filled, protocol.UseItemActionClickBlock, protocol.BlockPos{13, 71, -29}, 5, 7,
		mgl32.Vec3{13.25, 72.625, -28.75}, mgl32.Vec3{0.125, 0.875, 0.625}, 123_456)
	filledData := filled.TransactionData.(*protocol.UseItemTransactionData)
	if filledData.HeldItem.Stack.NetworkID != 5 || filledData.HeldItem.Stack.Count != 2 || filledData.HeldItem.StackNetworkID != 11 {
		t.Fatalf("filled held item = %+v, want network=5 count=2 stack=11", filledData.HeldItem)
	}

	empty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_click_block_empty_hand.bin"))
	assertBlockUseTransaction(t, empty, protocol.UseItemActionClickBlock, protocol.BlockPos{-8, 63, 21}, 0, 0,
		mgl32.Vec3{-7.75, 64.5, 21.875}, mgl32.Vec3{0.75, 0.25, 0.5}, ^uint32(0))
	emptyData := empty.TransactionData.(*protocol.UseItemTransactionData)
	if emptyData.HeldItem.Stack.NetworkID != 0 || emptyData.HeldItem.Stack.Count != 0 || emptyData.HeldItem.StackNetworkID != 0 {
		t.Fatalf("empty held item = %+v, want canonical empty item", emptyData.HeldItem)
	}

	destroy := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_destroy_block.bin"))
	assertBlockUseTransaction(t, destroy, protocol.UseItemActionBreakBlock, protocol.BlockPos{24, 68, -41}, 3, 5,
		mgl32.Vec3{24.625, 69.5, -40.125}, mgl32.Vec3{0.625, 0.375, 0.875}, 654_321)
	assertFixtureItem(t, destroy.TransactionData.(*protocol.UseItemTransactionData).HeldItem, 9, 3, 15)

	destroyEmpty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_destroy_block_empty_hand.bin"))
	assertBlockUseTransaction(t, destroyEmpty, protocol.UseItemActionBreakBlock, protocol.BlockPos{-17, 92, 6}, 1, 0,
		mgl32.Vec3{-16.5, 93.625, 6.25}, mgl32.Vec3{0.5, 1, 0.25}, ^uint32(0))
	assertFixtureItem(t, destroyEmpty.TransactionData.(*protocol.UseItemTransactionData).HeldItem, 0, 0, 0)

	attack := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_attack_actor.bin"))
	assertActorUseTransaction(t, attack, 0x0102_0304_0506_0708, protocol.UseItemOnEntityActionAttack,
		8, mgl32.Vec3{10.25, 65.625, -4.75}, mgl32.Vec3{0.375, 1.25, -0.125})
	assertFixtureItem(t, attack.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 7, 1, 13)

	attackEmpty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_attack_actor_empty_hand.bin"))
	assertActorUseTransaction(t, attackEmpty, ^uint64(0), protocol.UseItemOnEntityActionAttack,
		0, mgl32.Vec3{-12.5, 70, 31.75}, mgl32.Vec3{-0.5, 0.625, 1.5})
	assertFixtureItem(t, attackEmpty.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 0, 0, 0)

	interact := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_interact_actor.bin"))
	assertActorUseTransaction(t, interact, 123_456_789, protocol.UseItemOnEntityActionInteract,
		3, mgl32.Vec3{2.5, 63.875, 9.125}, mgl32.Vec3{0.25, 0.75, 0.5})
	assertFixtureItem(t, interact.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 8, 4, 14)

	interactEmpty := decodeClientFixture(t, filepath.Join(out, "inventory_transaction_interact_actor_empty_hand.bin"))
	assertActorUseTransaction(t, interactEmpty, 1, protocol.UseItemOnEntityActionInteract,
		6, mgl32.Vec3{-1.25, 80.5, -16.75}, mgl32.Vec3{1.125, -0.25, 0.875})
	assertFixtureItem(t, interactEmpty.TransactionData.(*protocol.UseItemOnEntityTransactionData).HeldItem, 0, 0, 0)

	closed := decodeClientPacket(t, filepath.Join(out, "container_close.bin"))
	closePacket, ok := closed.(*packet.ContainerClose)
	if !ok {
		t.Fatalf("container close decoded as %T", closed)
	}
	if closePacket.WindowID != 5 || closePacket.ContainerType != 0 || closePacket.ServerSide {
		t.Fatalf("container close = %+v, want client close for window 5/type 0", closePacket)
	}
}

func TestEmbeddedFilledUseHasExactStackAndNoPredictedInventoryActions(t *testing.T) {
	out := t.TempDir()
	if err := generate(out); err != nil {
		t.Fatal(err)
	}
	decoded := decodeClientPacket(t, filepath.Join(out, "player_auth_input_use_block.bin"))
	pk, ok := decoded.(*packet.PlayerAuthInput)
	if !ok {
		t.Fatalf("decoded %T, want PlayerAuthInput", decoded)
	}
	data, present := pk.ItemInteractionData.Value()
	if !present {
		t.Fatal("missing embedded item interaction")
	}
	if _, present := pk.BlockActions.Value(); present {
		t.Fatal("unexpected block actions")
	}
	if len(data.Actions) != 0 || data.Hand != protocol.HandSlotMainHand {
		t.Fatal("unexpected inventory prediction or off-hand use")
	}
	if data.ActionType != protocol.UseItemActionClickBlock || data.TriggerType != protocol.TriggerTypePlayerInput ||
		data.BlockPosition != (protocol.BlockPos{13, 71, -29}) || data.BlockFace != 5 || data.HotBarSlot != 7 ||
		data.Position != (mgl32.Vec3{13.25, 72.625, -28.75}) || data.ClickedPosition != (mgl32.Vec3{0.125, 0.875, 0.625}) ||
		data.BlockRuntimeID != 123456 || data.ClientPrediction != protocol.ClientPredictionFailure || data.ClientCooldownState != protocol.ClientCooldownStateOff {
		t.Fatalf("unexpected Use data: %+v", data)
	}
	assertFixtureItem(t, data.HeldItem, 5, 37, 41)
	if uint32(data.HeldItem.Stack.BlockRuntimeID) != 0x87654321 || data.HeldItem.Stack.MetadataValue != 3 || data.HeldItem.Stack.NBTData["fixture"] != int32(1) {
		t.Fatalf("held stack identity/data changed: %+v", data.HeldItem)
	}
}

func assertActorUseTransaction(t *testing.T, transaction *packet.InventoryTransaction, runtimeID uint64,
	action, slot int32, player, hit mgl32.Vec3) {
	t.Helper()
	if transaction.LegacyRequestID != 0 || len(transaction.LegacySetItemSlots) != 0 || len(transaction.Actions) != 0 {
		t.Fatalf("legacy/actions = (%d, %d, %d), want all empty", transaction.LegacyRequestID,
			len(transaction.LegacySetItemSlots), len(transaction.Actions))
	}
	data, ok := transaction.TransactionData.(*protocol.UseItemOnEntityTransactionData)
	if !ok {
		t.Fatalf("transaction data = %T, want use-item-on-entity", transaction.TransactionData)
	}
	if data.TargetEntityRuntimeID != runtimeID || data.ActionType != action || data.HotBarSlot != slot ||
		data.Position != player || data.ClickedPosition != hit {
		t.Fatalf("actor-use data = %+v", data)
	}
}

func assertFixtureItem(t *testing.T, item protocol.ItemInstance, networkID int32, count uint16, stackNetworkID int32) {
	t.Helper()
	if item.Stack.NetworkID != networkID || item.Stack.Count != count || item.StackNetworkID != stackNetworkID {
		t.Fatalf("held item = %+v, want network=%d count=%d stack=%d", item, networkID, count, stackNetworkID)
	}
}

func decodeClientFixture(t *testing.T, path string) *packet.InventoryTransaction {
	t.Helper()
	decoded := decodeClientPacket(t, path)
	transaction, ok := decoded.(*packet.InventoryTransaction)
	if !ok {
		t.Fatalf("inventory transaction decoded as %T", decoded)
	}
	return transaction
}

func decodeClientPacket(t *testing.T, path string) packet.Packet {
	t.Helper()
	raw := readFile(t, path)
	entries, err := packet.NewDecoder(bytes.NewReader(raw)).Decode()
	if err != nil {
		t.Fatalf("decode raw batch %s: %v", filepath.Base(path), err)
	}
	if len(entries) != 1 {
		t.Fatalf("%s entries = %d, want 1", filepath.Base(path), len(entries))
	}
	body := bytes.NewBuffer(entries[0])
	var header packet.Header
	if err := header.Read(body); err != nil {
		t.Fatalf("decode %s header: %v", filepath.Base(path), err)
	}
	constructor, ok := packet.NewClientPool()[header.PacketID]
	if !ok {
		t.Fatalf("packet %d is absent from client pool", header.PacketID)
	}
	decoded := constructor()
	decoded.Marshal(protocol.NewReader(body, 0, true))
	if body.Len() != 0 {
		t.Fatalf("%s leaves %d trailing bytes", filepath.Base(path), body.Len())
	}
	return decoded
}

func assertBlockUseTransaction(t *testing.T, transaction *packet.InventoryTransaction, action uint32, block protocol.BlockPos,
	face, slot int32, player, click mgl32.Vec3, runtimeID uint32) {
	t.Helper()
	if transaction.LegacyRequestID != 0 || len(transaction.LegacySetItemSlots) != 0 || len(transaction.Actions) != 0 {
		t.Fatalf("legacy/actions = (%d, %d, %d), want all empty", transaction.LegacyRequestID,
			len(transaction.LegacySetItemSlots), len(transaction.Actions))
	}
	data, ok := transaction.TransactionData.(*protocol.UseItemTransactionData)
	if !ok {
		t.Fatalf("transaction data = %T, want use-item", transaction.TransactionData)
	}
	if data.ActionType != action || data.TriggerType != protocol.TriggerTypePlayerInput ||
		data.BlockPosition != block || data.BlockFace != face || data.HotBarSlot != slot || data.Position != player ||
		data.ClickedPosition != click || data.BlockRuntimeID != runtimeID ||
		data.ClientPrediction != protocol.ClientPredictionFailure || data.ClientCooldownState != protocol.ClientCooldownStateOff {
		t.Fatalf("click-block data = %+v", data)
	}
}

func TestMiningInputHasOneBoundedRequestAndIndependentOptionalPrediction(t *testing.T) {
	out := t.TempDir()
	if err := generate(out); err != nil {
		t.Fatalf("generate: %v", err)
	}
	for _, name := range []string{"player_auth_input_mine_block.bin", "player_auth_input_mine_block_and_predict.bin"} {
		pk, ok := decodeClientPacket(t, filepath.Join(out, name)).(*packet.PlayerAuthInput)
		if !ok {
			t.Fatalf("%s is not PlayerAuthInput", name)
		}
		request, present := pk.ItemStackRequest.Value()
		if !present || request.RequestID != -3 || len(request.Actions) != 1 || len(request.FilterStrings) != 0 || request.FilterCause != -1 {
			t.Fatalf("%s request shape differs", name)
		}
		action, ok := request.Actions[0].(*protocol.MineBlockStackRequestAction)
		if !ok || action.HotbarSlot != 2 || action.PredictedDurability != 7 || action.StackNetworkID != 12345 {
			t.Fatalf("%s mining action differs", name)
		}
		var encoded bytes.Buffer
		var stackAction protocol.StackRequestAction = action
		protocol.NewWriter(&encoded, 0).StackRequestAction(&stackAction)
		if encoded.Len() < 2 || !bytes.Equal(encoded.Bytes()[:2], []byte{9, 11}) {
			t.Fatalf("%s outer/inner mining tags differ", name)
		}
		predict := name == "player_auth_input_mine_block_and_predict.bin"
		if !pk.InputData.Load(packet.InputFlagPerformItemStackRequest) || pk.InputData.Load(packet.InputFlagPerformBlockActions) != predict {
			t.Fatalf("%s derived flags differ", name)
		}
		actions, present := pk.BlockActions.Value()
		if present != predict || (predict && (len(actions) != 1 || actions[0].Action != protocol.PlayerActionPredictDestroyBlock)) {
			t.Fatalf("%s optional prediction differs", name)
		}
	}
}

func TestGenerateIsDeterministicAndWritesPinnedRawBatches(t *testing.T) {
	firstDir := t.TempDir()
	secondDir := t.TempDir()
	if err := generate(firstDir); err != nil {
		t.Fatalf("generate first corpus: %v", err)
	}
	if err := generate(secondDir); err != nil {
		t.Fatalf("generate second corpus: %v", err)
	}

	firstManifestBytes := readFile(t, filepath.Join(firstDir, "manifest.json"))
	secondManifestBytes := readFile(t, filepath.Join(secondDir, "manifest.json"))
	if !bytes.Equal(firstManifestBytes, secondManifestBytes) {
		t.Fatal("manifest differs between identical generator runs")
	}
	if len(firstManifestBytes) == 0 || firstManifestBytes[len(firstManifestBytes)-1] != '\n' {
		t.Fatal("manifest must end in exactly one newline")
	}

	var manifest []testManifestEntry
	if err := json.Unmarshal(firstManifestBytes, &manifest); err != nil {
		t.Fatalf("decode manifest: %v", err)
	}
	wantNames := []string{
		"ItemStackRequestPartialMoves",
		"NetworkSettings",
		"StartGame",
		"LevelChunk",
		"MovePlayer",
		"PlayerAuthInput",
		"PlayerAuthInputBlockActions",
		"PlayerAuthInputBreakBlock",
		"PlayerAuthInputUseBlock",
		"PlayerAuthInputBlockActionsAndBreakBlock",
		"PlayerAuthInputMineBlock",
		"PlayerAuthInputMineBlockAndPredict",
		"AddActor",
		"Text",
		"TextObjectRawText",
		"TextObjectWhisperRawText",
		"TextObjectAnnouncementRawText",
		"SetTitle",
		"BossEvent",
		"ModalFormRequest",
		"ModalFormTextMenu",
		"ModalFormResponseButton",
		"ModalFormResponseClosed",
		"ModalFormResponseBusy",
		"AvailableCommands",
		"AvailableCommandsLive356513",
		"BiomeDefinitionListChunkGeneration",
		"InventoryContent",
		"InventorySlot",
		"PlayerHotBar",
		"ItemStackResponse",
		"InventoryTransactionClickBlock",
		"InventoryTransactionClickBlockEmptyHand",
		"InventoryTransactionDestroyBlock",
		"InventoryTransactionDestroyBlockEmptyHand",
		"InventoryTransactionAttackActor",
		"InventoryTransactionAttackActorEmptyHand",
		"InventoryTransactionInteractActor",
		"InventoryTransactionInteractActorEmptyHand",
		"ContainerClose",
		"PlaySound",
		"StopSound",
		"LevelSoundEvent",
		"DisconnectVisible",
		"DisconnectFiltered",
		"DisconnectHidden",
		"CraftingDataManualNamed1x1",
		"CraftingDataManualNamed1x2",
		"CraftingDataManualUnsupportedReplacement",
		"CraftingDataManualClearEmpty",
		"ItemStackRequestManualCraft",
		"ItemStackResponseManualCraft",
	}
	wantIDs := []uint32{147, 143, 11, 58, 19, 144, 144, 144, 144, 144, 144, 144, 13, 9, 9, 9, 9, 88, 74, 100, 100, 101, 101, 101, 76, 76, 122, 49, 50, 48, 148, 30, 30, 30, 30, 30, 30, 30, 30, 47, 86, 87, 123, 5, 5, 5, 52, 52, 52, 52, 147, 148}
	wantHeaders := [][]byte{
		{0x93, 0x49},
		{0x8f, 0x49},
		{0x8b, 0x48},
		{0xba, 0x48},
		{0x93, 0x48},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x90, 0x49},
		{0x8d, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0x89, 0x48},
		{0xd8, 0x48},
		{0xca, 0x48},
		{0xe4, 0x48},
		{0xe4, 0x48},
		{0xe5, 0x48},
		{0xe5, 0x48},
		{0xe5, 0x48},
		{0xcc, 0x48},
		{0xcc, 0x48},
		{0xfa, 0x48},
		{0xb1, 0x48},
		{0xb2, 0x48},
		{0xb0, 0x48},
		{0x94, 0x49},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0x9e, 0x48},
		{0xaf, 0x48},
		{0xd6, 0x48},
		{0xd7, 0x48},
		{0xfb, 0x48},
		{0x85, 0x48},
		{0x85, 0x48},
		{0x85, 0x48},
		{0xb4, 0x48},
		{0xb4, 0x48},
		{0xb4, 0x48},
		{0xb4, 0x48},
		{0x93, 0x49},
		{0x94, 0x49},
	}
	if len(wantNames) != len(wantIDs) || len(wantNames) != len(wantHeaders) {
		t.Fatalf("expected corpus cardinality differs: names=%d IDs=%d headers=%d", len(wantNames), len(wantIDs), len(wantHeaders))
	}
	if len(manifest) != len(wantNames) {
		t.Fatalf("manifest entries = %d, want %d", len(manifest), len(wantNames))
	}

	for i, entry := range manifest {
		if entry.Name != wantNames[i] || entry.ID != wantIDs[i] {
			t.Fatalf("entry %d identity = (%q, %d), want (%q, %d)", i, entry.Name, entry.ID, wantNames[i], wantIDs[i])
		}
		first := readFile(t, filepath.Join(firstDir, entry.File))
		second := readFile(t, filepath.Join(secondDir, entry.File))
		if !bytes.Equal(first, second) {
			t.Fatalf("%s differs between identical generator runs", entry.Name)
		}
		if len(first) != entry.ByteLength {
			t.Fatalf("%s byte length = %d, manifest says %d", entry.Name, len(first), entry.ByteLength)
		}
		digest := sha256.Sum256(first)
		if got := hex.EncodeToString(digest[:]); got != entry.SHA256 {
			t.Fatalf("%s sha256 = %s, manifest says %s", entry.Name, got, entry.SHA256)
		}
		if len(first) < 2 || first[0] != 0xfe {
			t.Fatalf("%s does not begin with raw batch header 0xfe", entry.Name)
		}

		payload := bytes.NewBuffer(first[1:])
		var declared uint32
		if err := protocol.Varuint32(payload, &declared); err != nil {
			t.Fatalf("%s length varuint: %v", entry.Name, err)
		}
		if int(declared) != payload.Len() {
			t.Fatalf("%s declared entry length = %d, remaining = %d", entry.Name, declared, payload.Len())
		}
		if payload.Len() < len(wantHeaders[i]) {
			t.Fatalf("%s truncated packet header: %d bytes, want at least %d", entry.Name, payload.Len(), len(wantHeaders[i]))
		}
		if got := payload.Bytes()[:len(wantHeaders[i])]; !reflect.DeepEqual(got, wantHeaders[i]) {
			t.Fatalf("%s header bytes = %x, want %x", entry.Name, got, wantHeaders[i])
		}
		if entry.Name == "AvailableCommandsLive356513" {
			const packetHeaderBytes = 2
			if got := payload.Len() - packetHeaderBytes; got != 356_513 {
				t.Fatalf("live AvailableCommands body length = %d, want 356513", got)
			}
		}
		if entry.Name == "BiomeDefinitionListChunkGeneration" {
			if entry.ByteLength != 48 {
				t.Fatalf("biome definition fixture length = %d, want 48", entry.ByteLength)
			}
			if entry.SHA256 != "a1a626d9b27cd943bc38fbbc356a09ea711ddb26acad72e284dd8dfaff94fbd4" {
				t.Fatalf("biome definition fixture sha256 = %s", entry.SHA256)
			}
			if entry.WireAuthority != "hashimthearab/gophertunnel" || entry.WireCommit != "9f42f3679a573fc4b51104569cc4f422036e28ec" {
				t.Fatalf("biome definition fixture provenance = (%q, %q)", entry.WireAuthority, entry.WireCommit)
			}
		}
		if entry.Name == "InventoryTransactionDestroyBlock" || entry.Name == "InventoryTransactionDestroyBlockEmptyHand" {
			if entry.WireAuthority != "hashimthearab/gophertunnel" || entry.WireCommit != "b725d82563e93308fd1f92d27da5e97301ad5040" {
				t.Fatalf("destroy-block fixture provenance = (%q, %q)", entry.WireAuthority, entry.WireCommit)
			}
		}
	}
}

func readFile(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return b
}
