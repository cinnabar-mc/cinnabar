package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
)

func writeBlockItemRouteTable(output, bregPath string) error {
	if _, err := collect(world.DefaultBlockRegistry); err != nil {
		return fmt.Errorf("initialize block registry: %w", err)
	}
	breg, err := os.ReadFile(bregPath)
	if err != nil {
		return fmt.Errorf("read block-item BREG: %w", err)
	}
	if len(breg) > 128<<20 {
		return errors.New("block-item BREG exceeds 128 MiB")
	}
	if len(breg) < 12 {
		return errors.New("block-item BREG header is truncated")
	}
	protocol := binary.LittleEndian.Uint32(breg[8:12])
	records, err := readBREG1003IdentitiesForProtocol(breg, protocol)
	if err != nil {
		return err
	}
	table, err := generateBlockItemRouteTable(world.Items(), records, breg, world.DefaultBlockRegistry, protocol, protocol == v2225BlockProtocol)
	if err != nil {
		return err
	}
	encoded, err := encodeBlockItemRouteTable(table, protocol)
	if err != nil {
		return fmt.Errorf("encode block-item routes: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(output), 0o755); err != nil {
		return fmt.Errorf("create block-item output directory: %w", err)
	}
	if err := os.WriteFile(output, encoded, 0o644); err != nil {
		return fmt.Errorf("write block-item routes: %w", err)
	}
	return nil
}

func encodeBlockItemRouteTable(table BlockItemRouteTable, protocol uint32) ([]byte, error) {
	encoded, err := json.MarshalIndent(table, "", "  ")
	if err != nil {
		return nil, err
	}
	encoded = append(encoded, '\n')
	if protocol == v2225BlockProtocol {
		// This semantic-neutral byte revision forces existing autocrlf checkouts
		// to rematerialize the active carrier after its tracked-LF pin lands.
		encoded = bytes.Replace(encoded, []byte(`"schema":`), []byte(`"schema" :`), 1)
	}
	return encoded, nil
}

// generateBlockItemRouteTable routes Dragonfly block items onto BREG states.
// With skipReservedNames, an item whose block has no retained BREG state is
// skipped; otherwise the protocol-1001 reserved ranges exclude items.
func generateBlockItemRouteTable(items []world.Item, records []bregLightIdentity, breg []byte, registry world.BlockRegistry, protocol uint32, skipReservedNames bool) (BlockItemRouteTable, error) {
	index := make(map[string][]bregLightIdentity, len(records))
	retainedNames := make(map[string]struct{}, len(records))
	for _, record := range records {
		retainedNames[record.Name] = struct{}{}
		if record.SequentialID >= uint32(len(records)) {
			return BlockItemRouteTable{}, fmt.Errorf("block visual %d is out of range", record.SequentialID)
		}
		key := canonicalRecordKey(record.Name, record.StateJSON)
		index[key] = append(index[key], record)
	}
	routes := make([]BlockItemRoute, 0, len(items))
	seen := make(map[string]struct{}, len(items))
	for _, candidate := range items {
		value, ok := candidate.(world.Block)
		if !ok {
			continue
		}
		if skipReservedNames {
			if blockName, _ := value.EncodeBlock(); !slicesContainsKey(retainedNames, canonicalBlockName(blockName)) {
				continue
			}
		} else if runtimeID, ok := registeredBlockRuntimeID(registry, value); ok && isRetailReservedSequentialID(runtimeID) {
			continue
		}
		identifier, metadata := candidate.EncodeItem()
		if identifier == "" || canonicalBlockName(identifier) != identifier || !strings.HasPrefix(identifier, "minecraft:") {
			return BlockItemRouteTable{}, fmt.Errorf("noncanonical block item identifier %q", identifier)
		}
		metadata32 := uint32(int32(metadata))
		itemKey := fmt.Sprintf("%s\x00%d", identifier, metadata32)
		if _, exists := seen[itemKey]; exists {
			return BlockItemRouteTable{}, fmt.Errorf("duplicate block item route %s metadata %d", identifier, metadata32)
		}
		seen[itemKey] = struct{}{}
		blockName, properties := value.EncodeBlock()
		typed, err := typedProperties(properties)
		if err != nil {
			return BlockItemRouteTable{}, fmt.Errorf("block item %s metadata %d: %w", identifier, metadata32, err)
		}
		state, err := canonicalTypedState(typed)
		if err != nil {
			return BlockItemRouteTable{}, fmt.Errorf("block item %s metadata %d: %w", identifier, metadata32, err)
		}
		matches := index[canonicalRecordKey(blockName, state)]
		if len(matches) != 1 {
			return BlockItemRouteTable{}, fmt.Errorf("block item %s metadata %d resolves to %d canonical states", identifier, metadata32, len(matches))
		}
		routes = append(routes, BlockItemRoute{
			Identifier: identifier, Metadata: metadata32, BlockName: blockName,
			BlockState: append(json.RawMessage(nil), state...), BlockVisual: matches[0].SequentialID,
		})
	}
	sort.Slice(routes, func(i, j int) bool {
		if routes[i].Identifier != routes[j].Identifier {
			return routes[i].Identifier < routes[j].Identifier
		}
		return routes[i].Metadata < routes[j].Metadata
	})
	digest := sha256.Sum256(breg)
	return BlockItemRouteTable{
		Schema: 1, Protocol: protocol, CanonicalBlockStates: uint32(len(records)),
		DragonflyModule: dragonflyModule, DragonflyVersion: dragonflyVersion,
		DragonflyModuleSum: dragonflyModuleSum, BREGSHA256: fmt.Sprintf("%x", digest), Routes: routes,
	}, nil
}

func registeredBlockRuntimeID(registry world.BlockRegistry, block world.Block) (runtimeID uint32, ok bool) {
	if registry == nil {
		return 0, false
	}
	defer func() {
		if recover() != nil {
			runtimeID, ok = 0, false
		}
	}()
	return registry.BlockRuntimeID(block), true
}
