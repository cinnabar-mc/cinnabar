package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"sort"
	"strings"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
	sharedblock "github.com/cinnabar-mc/protocolgen/generated/data/block"
	"github.com/cinnabar-mc/protocolgen/generated/data/registry"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
	"github.com/segmentio/fasthash/fnv1"
	"github.com/segmentio/fasthash/fnv1a"
)

const (
	v2225BlockProtocol       = shared.ProtocolVersion
	v2225GameVersion         = shared.MinecraftVersion
	v2225RetailItemsPath     = "crates/protocol/data/retail_items_1_26_60.tsv"
	v2225RetailItemsSHA256   = "4d9b95a973180279fba6c45e115c5327f995d5b50ac828aad278d6ca2a16946c"
	v2225BlockOutputPath     = "crates/assets/data/block-registry-v2225.bin"
	v2225LightOutputPath     = "crates/assets/data/block-light-registry-v2225.bin"
	v2225BlockManifestSchema = "cinnabar.block-projection.v2"
)

var v2225BlockStateCount = sharedblock.StateCount()

type v2225SourceEntry struct {
	state   world.BlockState
	ordinal int
}

func orderV2225BlockStates(states []world.BlockState) []v2225SourceEntry {
	ordered := make([]v2225SourceEntry, len(states))
	for index, state := range states {
		ordered[index] = v2225SourceEntry{state: state, ordinal: index}
	}
	sort.SliceStable(ordered, func(i, j int) bool {
		return fnv1.HashString64(ordered[i].state.Name) < fnv1.HashString64(ordered[j].state.Name)
	})
	return ordered
}

// readV2225BlockStates reads the palette from the shared authenticated catalog.
func readV2225BlockStates() ([]world.BlockState, error) {
	if err := validateSharedTarget(); err != nil {
		return nil, err
	}
	return decodeSharedBlockStates(registry.BlockStatesNBT())
}

// decodeSharedBlockStates rejects truncated compounds and unexpected palette sizes.
func decodeSharedBlockStates(data []byte) ([]world.BlockState, error) {
	reader := bytes.NewReader(data)
	decoder := nbt.NewDecoder(reader)
	states := make([]world.BlockState, 0, v2225BlockStateCount)
	for reader.Len() != 0 {
		var state world.BlockState
		err := decoder.Decode(&state)
		if err != nil {
			return nil, fmt.Errorf("decode pinned block state %d: %w", len(states), err)
		}
		states = append(states, state)
	}
	if len(states) != v2225BlockStateCount {
		return nil, fmt.Errorf("pinned block-state source contains %d states, want %d", len(states), v2225BlockStateCount)
	}
	return states, nil
}

// v2225SourceRecords converts shared palette states to the client carrier identity format.
func v2225SourceRecords() ([]Record, error) {
	states, err := readV2225BlockStates()
	if err != nil {
		return nil, err
	}
	// Reproduce Finalize's authoritative stable FNV-1 identifier ordering while
	// preserving source order among states with the same identifier.
	ordered := orderV2225BlockStates(states)
	records := make([]Record, len(ordered))
	seenHashes := make(map[uint32]struct{}, len(ordered))
	for rid, entry := range ordered {
		name, properties := entry.state.Name, entry.state.Properties
		typed, err := typedProperties(properties)
		if err != nil {
			return nil, fmt.Errorf("Dragonfly runtime ID %d: %w", rid, err)
		}
		canonical, err := canonicalTypedState(typed)
		if err != nil {
			return nil, fmt.Errorf("canonicalize Dragonfly runtime ID %d: %w", rid, err)
		}
		networkHash, err := v2225NetworkBlockHash(name, properties)
		if err != nil {
			return nil, fmt.Errorf("network hash for runtime ID %d: %w", rid, err)
		}
		if _, exists := seenHashes[networkHash]; exists {
			return nil, fmt.Errorf("duplicate network hash at runtime ID %d", rid)
		}
		seenHashes[networkHash] = struct{}{}
		records[rid] = Record{SequentialID: uint32(rid), NetworkHash: networkHash, Name: name, StateJSON: canonical, Provenance: ProvenanceDragonfly}
	}
	return records, nil
}

func v2225NetworkBlockHash(name string, properties map[string]any) (uint32, error) {
	if name == "minecraft:unknown" {
		return math.MaxUint32 - 1, nil
	}
	keys := make([]string, 0, len(properties))
	for key := range properties {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	data := []byte{10, 0, 0}
	writeString := func(value string) {
		data = binary.LittleEndian.AppendUint16(data, uint16(len(value)))
		data = append(data, value...)
	}
	data = append(data, 8)
	writeString("name")
	writeString(name)
	data = append(data, 10)
	writeString("states")
	for _, key := range keys {
		switch value := properties[key].(type) {
		case string:
			data = append(data, 8)
			writeString(key)
			writeString(value)
		case uint8:
			data = append(data, 1)
			writeString(key)
			data = append(data, value)
		case int8:
			data = append(data, 1)
			writeString(key)
			data = append(data, byte(value))
		case bool:
			data = append(data, 1)
			writeString(key)
			if value {
				data = append(data, 1)
			} else {
				data = append(data, 0)
			}
		case uint16:
			data = append(data, 2)
			writeString(key)
			data = binary.LittleEndian.AppendUint16(data, value)
		case int16:
			data = append(data, 2)
			writeString(key)
			data = binary.LittleEndian.AppendUint16(data, uint16(value))
		case uint32:
			data = append(data, 3)
			writeString(key)
			data = binary.LittleEndian.AppendUint32(data, value)
		case int32:
			data = append(data, 3)
			writeString(key)
			data = binary.LittleEndian.AppendUint32(data, uint32(value))
		default:
			return 0, fmt.Errorf("unsupported property %q type %T", key, value)
		}
	}
	data = append(data, 0, 0)
	return fnv1a.HashBytes32(data), nil
}

func parseV2225RetailItems(path string) (map[string]struct{}, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read retail item allowlist: %w", err)
	}
	if fmt.Sprintf("%x", sha256.Sum256(data)) != v2225RetailItemsSHA256 {
		return nil, errors.New("retail item allowlist identity does not match")
	}
	allowed := make(map[string]struct{})
	for index, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
		fields := strings.Split(strings.TrimSuffix(line, "\r"), "\t")
		if len(fields) != 2 || !strings.HasPrefix(fields[1], "minecraft:") {
			return nil, fmt.Errorf("retail item allowlist row %d is malformed", index)
		}
		if _, exists := allowed[fields[1]]; exists {
			return nil, fmt.Errorf("retail item allowlist row %d duplicates an identifier", index)
		}
		allowed[fields[1]] = struct{}{}
	}
	return allowed, nil
}

// v2225IsConnectionProperty identifies neighbour-derived keys removed for the legacy fact join.
func v2225IsConnectionProperty(name string) bool {
	switch name {
	case "minecraft:corner", "redstone_east", "redstone_north", "redstone_south", "redstone_west":
		return true
	default:
		return strings.HasPrefix(name, "minecraft:connection_")
	}
}

func v2225ReducedState(state []byte) ([]byte, error) {
	var properties map[string]json.RawMessage
	if err := json.Unmarshal(state, &properties); err != nil {
		return nil, fmt.Errorf("decode canonical state: %w", err)
	}
	for name := range properties {
		if v2225IsConnectionProperty(name) {
			delete(properties, name)
		}
	}
	return json.Marshal(properties)
}

func decodeBREGRecords(data []byte, expectedProtocol uint32) (RegistryMetadata, []Record, error) {
	const headerBytes = registryHeaderBytes
	if len(data) < headerBytes || (string(data[:8]) != registryHeader && string(data[:8]) != previousRegistryHeader) || binary.LittleEndian.Uint32(data[8:12]) != expectedProtocol {
		return RegistryMetadata{}, nil, fmt.Errorf("input is not protocol-%d BREG1003", expectedProtocol)
	}
	previous := string(data[:8]) == previousRegistryHeader
	prefixBytes := recordHeaderBytes
	if previous {
		prefixBytes = previousRecordHeaderBytes
	}
	metadata := decodeRegistryMetadata(data[len(registryHeader):headerBytes])
	if metadata.CanonicalStates > maxRecordCount {
		return RegistryMetadata{}, nil, errors.New("BREG record count exceeds limit")
	}
	records := make([]Record, 0, metadata.CanonicalStates)
	cursor := headerBytes
	for index := uint32(0); index < metadata.CanonicalStates; index++ {
		if len(data)-cursor < prefixBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d is truncated", index)
		}
		p := decodeRecordHeader(data[cursor:cursor+prefixBytes], previous)
		boxCount := int(p.BoxCount)
		nameLen, stateLen := int(p.NameLen), int(p.StateLen)
		if boxCount > maxCollisionBoxesPerRecord || stateLen > maxStateBytes {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d exceeds bounds", index)
		}
		payload := cursor + prefixBytes + boxCount*collisionBoxBytes
		end := payload + nameLen + stateLen
		if payload < cursor || end < payload || end > len(data) {
			return RegistryMetadata{}, nil, fmt.Errorf("BREG record %d payload is truncated", index)
		}
		record := Record{
			SequentialID: p.SequentialID, NetworkHash: p.NetworkHash, Flags: p.RawFlags,
			ModelFamily: ModelFamily(p.ModelFamily), ContributorRole: ContributorRole(p.ContributorRole),
			ModelState: ModelState{Mask: p.ModelMask, Values: p.Values}, FaceCoverage: p.FaceCoverage,
			CollisionSeed: CollisionSeed{Confidence: CollisionConfidence(p.Confidence), ShapeID: p.ShapeID},
			Provenance:    p.RawProvenance, Name: string(data[payload : payload+nameLen]),
			StateJSON:  append([]byte(nil), data[payload+nameLen:end]...),
			BeaconTint: BeaconTint(p.BeaconTint),
			Traits:     p.RawTraits, Survival: SurvivalRule(p.Survival),
		}
		for boxIndex := 0; boxIndex < boxCount; boxIndex++ {
			start := cursor + prefixBytes + boxIndex*collisionBoxBytes
			record.CollisionSeed.Boxes = append(record.CollisionSeed.Boxes, decodeCollisionBox(data[start:start+collisionBoxBytes]))
		}
		records = append(records, record)
		cursor = end
	}
	if cursor != len(data) {
		return RegistryMetadata{}, nil, fmt.Errorf("BREG has %d trailing bytes", len(data)-cursor)
	}
	return metadata, records, nil
}

func decodeLREGProperties(data, breg []byte, expectedProtocol uint32, expectedCount int) ([]byte, error) {
	if expectedCount > maxRecordCount || len(data) != 48+expectedCount+sha256.Size || string(data[:8]) != lightRegistryHeader || binary.LittleEndian.Uint32(data[8:12]) != expectedProtocol || int(binary.LittleEndian.Uint32(data[12:16])) != expectedCount {
		return nil, fmt.Errorf("input is not protocol-%d LREG1001 with %d records", expectedProtocol, expectedCount)
	}
	bregDigest := sha256.Sum256(breg)
	if !bytes.Equal(data[16:48], bregDigest[:]) {
		return nil, errors.New("LREG BREG binding mismatch")
	}
	payloadEnd := 48 + expectedCount
	payloadDigest := sha256.Sum256(data[:payloadEnd])
	if !bytes.Equal(data[payloadEnd:], payloadDigest[:]) {
		return nil, errors.New("LREG payload digest mismatch")
	}
	return append([]byte(nil), data[48:payloadEnd]...), nil
}
