package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
)

// The physics seed combines shared state facts with client movement rules.

// v2225PhysicsReservedCount derives neutral-state expectations from the hash-bound admission inventory.
var v2225PhysicsReservedCount = sync.OnceValue(func() int {
	data, err := os.ReadFile(filepath.Join("..", "..", "crates", "assets", "data", "block-registry-v2225.bin"))
	if err != nil {
		panic(err)
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		panic(err)
	}
	if actual := fmt.Sprintf("%x", sha256.Sum256(data)); actual != blockHash {
		panic(fmt.Errorf("reserved-state inventory SHA-256 %s does not match %s", actual, blockHash))
	}
	_, records, err := decodeBREGRecords(data, v2225BlockProtocol)
	if err != nil {
		panic(err)
	}
	count := 0
	for _, record := range records {
		if record.Name == retailReservedName {
			count++
		}
	}
	return count
})

// writeV2225PhysicsProjection binds shared physics facts to the selected block carrier.
func writeV2225PhysicsProjectionOracle(bregPath, outputPath, shaOutputPath, manifestPath string) error {
	if bregPath == "" || outputPath == "" {
		return errors.New("v2225 physics projection requires a binding BREG and an output path")
	}
	data, err := os.ReadFile(bregPath)
	if err != nil {
		return fmt.Errorf("read v2225 physics-binding BREG: %w", err)
	}
	if len(data) > 128<<20 {
		return errors.New("v2225 physics-binding BREG exceeds 128 MiB")
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		return err
	}
	if actual := fmt.Sprintf("%x", sha256.Sum256(data)); actual != blockHash {
		return fmt.Errorf("pinned protocol-2225 block registry SHA-256 %s does not match %s", actual, blockHash)
	}
	_, records, err := decodeBREGRecords(data, v2225BlockProtocol)
	if err != nil {
		return err
	}
	physics, err := projectV2225PhysicsRecords(records)
	if err != nil {
		return err
	}
	if manifestPath != "" {
		payload, err := os.ReadFile(manifestPath)
		if err != nil {
			return fmt.Errorf("read v2225 physics projection manifest: %w", err)
		}
		if err := crossCheckV2225PhysicsManifest(payload); err != nil {
			return err
		}
	}
	encoded, err := encodePhysicsRegistryForProtocol(data, physics, v2225BlockStateCount, v2225BlockProtocol)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return fmt.Errorf("create v2225 physics output directory: %w", err)
	}
	if err := os.WriteFile(outputPath, encoded, 0o644); err != nil {
		return fmt.Errorf("write v2225 physics output: %w", err)
	}
	if shaOutputPath == "" {
		shaOutputPath = strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256"
	}
	digest := sha256.Sum256(encoded)
	if err := os.WriteFile(shaOutputPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		return fmt.Errorf("write v2225 physics checksum: %w", err)
	}
	return nil
}

// projectV2225PhysicsRecords uses exact shared state friction and collisions, with local movement behavior.
func projectV2225PhysicsRecords(records []Record) ([]PhysicsRecord, error) {
	if err := validateSharedTarget(); err != nil {
		return nil, err
	}
	if len(records) != v2225BlockStateCount {
		return nil, fmt.Errorf("v2225 physics record count %d does not match %d", len(records), v2225BlockStateCount)
	}
	physics := make([]PhysicsRecord, len(records))
	counts := make(map[string]int)
	reserved := 0
	for i, record := range records {
		if record.SequentialID != uint32(i) {
			return nil, fmt.Errorf("v2225 physics record %d has sequential ID %d", i, record.SequentialID)
		}
		entry := PhysicsRecord{SequentialID: record.SequentialID, NetworkHash: record.NetworkHash, FrictionQ1E8: defaultFrictionQ1E8, HorizontalSpeedQ1E8: defaultSpeedQ1E8, VerticalSpeedQ1E8: defaultSpeedQ1E8}
		if record.Name == retailReservedName {
			reserved++
			entry.Flags = physicsFlagPassable
			physics[i] = entry
			continue
		}
		counts[record.Name]++
		properties, err := sharedBlockProperties(record)
		if err != nil {
			return nil, err
		}
		seed, err := sharedCollisionSeed(record, properties)
		if err != nil {
			return nil, err
		}
		entry.Boxes = seed.Boxes
		entry.FrictionQ1E8, err = fixedPhysicsScalar(float64(properties.Friction))
		if err != nil {
			return nil, fmt.Errorf("shared friction for %s: %w", record.Name, err)
		}
		if len(entry.Boxes) == 0 {
			entry.Flags |= physicsFlagPassable
		}
		if override, ok := reviewedPhysicsOverrideFor(record.Name); ok {
			entry.MovementSemantics = MovementSemanticsSpecial
			if err := applyPhysicsOverride(record, override, &entry); err != nil {
				return nil, err
			}
		}
		physics[i] = entry
	}
	if reserved != v2225PhysicsReservedCount() {
		return nil, fmt.Errorf("v2225 physics projection found %d cinnabar:reserved states, want exactly %d", reserved, v2225PhysicsReservedCount())
	}
	for _, override := range reviewedPhysicsOverrides {
		if counts[override.Name] != override.StateCount {
			return nil, fmt.Errorf("reviewed physics override %s has %d states, want %d", override.Name, counts[override.Name], override.StateCount)
		}
	}
	return physics, nil
}

func crossCheckV2225PhysicsManifest(payload []byte) error {
	var manifest v2225BlockProjectionManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return fmt.Errorf("decode v2225 physics projection manifest: %w", err)
	}
	if manifest.Projection.DeniedCount != v2225PhysicsReservedCount() {
		return fmt.Errorf("v2225 projection manifest denies %d states, want exactly %d", manifest.Projection.DeniedCount, v2225PhysicsReservedCount())
	}
	if manifest.Projection.EducationStates != v2225EducationStateCount() {
		return fmt.Errorf("v2225 projection manifest has %d Education states, want %d", manifest.Projection.EducationStates, v2225EducationStateCount())
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		return err
	}
	if manifest.Output.SHA256 != blockHash {
		return fmt.Errorf("v2225 projection manifest binds BREG SHA-256 %q, want %q", manifest.Output.SHA256, blockHash)
	}
	return nil
}
