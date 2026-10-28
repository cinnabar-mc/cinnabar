package main

import (
	"crypto/sha256"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	_ "github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
	_ "github.com/df-mc/dragonfly/server/world/biome"
)

const (
	lightRegistryHeader = "LREG1001"
	biomeRegistryHeader = "BIOREG01"
	dragonflyModule     = "github.com/df-mc/dragonfly"
	dragonflyVersion    = "v0.11.5"
	dragonflyModuleSum  = "h1:amqepXVBRBi/e5j1K2H8GjNFgpMs6FP1RQgNH0Myfn0="

	collisionFixedScale   = 100_000_000.0
	collisionLocalHaloMin = -100_000_000
	collisionLocalHaloMax = 200_000_000

	maxBiomeRecordCount = 1_024
	maxBiomeNameBytes   = 256
)

type BlockItemRoute struct {
	Identifier  string          `json:"identifier"`
	Metadata    uint32          `json:"metadata"`
	BlockName   string          `json:"block_name"`
	BlockState  json.RawMessage `json:"block_state"`
	BlockVisual uint32          `json:"block_visual"`
}

type BlockItemRouteTable struct {
	Schema               uint32           `json:"schema"`
	Protocol             uint32           `json:"protocol"`
	CanonicalBlockStates uint32           `json:"canonical_block_states"`
	DragonflyModule      string           `json:"dragonfly_module"`
	DragonflyVersion     string           `json:"dragonfly_version"`
	DragonflyModuleSum   string           `json:"dragonfly_module_sum"`
	BREGSHA256           string           `json:"breg_sha256"`
	Routes               []BlockItemRoute `json:"routes"`
}

type lightRegistry interface {
	LightBlock(runtimeID uint32) uint8
	FilteringBlock(runtimeID uint32) uint8
}

var pmmpLightFallbackIdentifiers = []string{
	"minecraft:lit_redstone_lamp",
	"minecraft:redstone_lamp",
}

type PMMPLightProperties struct {
	Brightness float64 `json:"brightness"`
	Opacity    float64 `json:"opacity"`
	Friction   float64 `json:"friction"`
}

type LightGenerationReport struct {
	DragonflyRevision         string   `json:"dragonfly_revision"`
	DragonflyAccessorStates   int      `json:"dragonfly_accessor_states"`
	PMMPFallbackStates        int      `json:"pmmp_fallback_states"`
	PMMPFallbackIdentifiers   []string `json:"pmmp_fallback_identifiers"`
	PMMPFallbackSequentialIDs []uint32 `json:"pmmp_fallback_sequential_ids"`
	BREGSHA256                string   `json:"breg_sha256"`
}

type bregLightIdentity struct {
	SequentialID uint32
	NetworkHash  uint32
	Name         string
	StateJSON    []byte
}

type ScalarKind uint8

const (
	ScalarByte ScalarKind = iota + 1
	ScalarInt
	ScalarString
)

type TypedScalar struct {
	Kind   ScalarKind
	Byte   byte
	Int    int32
	String string
}

type StateProperty struct {
	Name  string
	Value TypedScalar
}

type SourceState struct {
	Name          string
	Properties    []StateProperty
	Ordinal       uint32
	NetworkHash   uint32
	Flags         uint8
	CollisionSeed CollisionSeed
}

const (
	modelFlagPowered uint32 = 1 << iota
	modelFlagPressed
	modelFlagAttached
	modelFlagHanging
	modelFlagHead
	modelFlagOccupied
	modelFlagInWall
	modelFlagUpper
)

func (s *ModelState) Set(field ModelStateField, value uint32) {
	if field == 0 || field > maxModelStateField {
		panic("invalid model state field")
	}
	bit := uint8(1 << (field - 1))
	s.Mask |= bit
	s.Values[field-1] = value
}

func (s ModelState) Get(field ModelStateField) (uint32, bool) {
	if field == 0 || field > maxModelStateField {
		return 0, false
	}
	bit := uint8(1 << (field - 1))
	return s.Values[field-1], s.Mask&bit != 0
}

type ValentineAudit struct {
	CanonicalNames  int      `json:"canonical_names"`
	CanonicalStates int      `json:"canonical_states"`
	ValentineNames  int      `json:"valentine_names"`
	ValentineStates int      `json:"valentine_states"`
	GapNames        int      `json:"gap_names"`
	GapStates       int      `json:"gap_states"`
	Joined          int      `json:"joined"`
	Missing         int      `json:"missing"`
	Extra           int      `json:"extra"`
	Mismatched      int      `json:"mismatched"`
	MissingNames    []string `json:"missing_names"`
	MissingStates   []string `json:"missing_states"`
}

type GenerationReport struct {
	Protocol               uint32                `json:"protocol"`
	CanonicalNames         int                   `json:"canonical_names"`
	CanonicalStates        int                   `json:"canonical_states"`
	ValentineAudit         ValentineAudit        `json:"valentine_audit"`
	PMMPPaletteSHA256      string                `json:"pmmp_palette_sha256"`
	PrismarineStateSHA256  string                `json:"prismarine_states_sha256"`
	PrismarineShapeSHA256  string                `json:"prismarine_shapes_sha256"`
	CoverageManifestSHA256 string                `json:"coverage_manifest_sha256"`
	LightMetadata          LightGenerationReport `json:"light_metadata"`
}

// BiomeRecord is one numeric biome ID and name in the client registry.
type BiomeRecord struct {
	ID   uint32
	Name string
}

func main() {
	factsOut := flag.String("facts-out", "", "export raw shared catalog facts for the Rust compiler")
	out := flag.String("out", "", "path to write the block registry")
	lightOut := flag.String("light-out", "", "path to write the BREG-bound block light registry")
	lightBREG := flag.String("light-breg", "", "existing reviewed BREG1003 whose exact bytes the light registry binds")
	physicsOut := flag.String("physics-out", "", "optional path to write the BREG-bound block physics registry")
	physicsSHAOut := flag.String("physics-sha-out", "", "optional path to write the physics registry SHA-256")
	physicsBREG := flag.String("physics-breg", "", "existing reviewed BREG1003 whose exact bytes the physics registry binds")
	physicsV2225Out := flag.String("physics-v2225-out", "", "path to write the projected protocol-2225 block physics registry")
	physicsV2225SHAOut := flag.String("physics-v2225-sha-out", "", "optional path to write the v2225 physics registry SHA-256")
	physicsV2225BREG := flag.String("physics-v2225-breg", "", "checked-in protocol-2225 BREG1003 whose exact bytes the physics projection binds")
	physicsV2225Manifest := flag.String("physics-v2225-manifest", "", "reviewed v2225 block-projection manifest cross-checked against the projection")
	biomeOut := flag.String("biome-out", "", "optional path to write the biome registry")
	biomeCoverage := flag.String("biome-coverage", "", "reviewed numeric biome coverage manifest")
	biomeV2225Allowlist := flag.String("biome-v2225-allowlist", "", "reviewed retail biome allowlist")
	biomeV2225Manifest := flag.String("biome-v2225-manifest", "", "path to write the v2225 projection manifest")
	blockV2225LegacyBREG := flag.String("block-v2225-legacy-breg", "", "reviewed protocol-1001 BREG used for conservative projection")
	blockV2225Allowlist := flag.String("block-v2225-allowlist", "", "reviewed retail item allowlist")
	blockV2225Manifest := flag.String("block-v2225-manifest", "", "path to write the v2225 block projection manifest")
	pmmpRoot := flag.String("pmmp", "", "pinned PMMP BedrockData directory")
	prismarineRoot := flag.String("prismarine", "", "pinned Prismarine minecraft-data directory")
	coverageManifest := flag.String("coverage", "", "reviewed numeric canonical-state coverage manifest")
	blockItemOut := flag.String("block-item-out", "", "optional reviewed block-item route JSON output")
	blockItemBREG := flag.String("block-item-breg", "", "existing reviewed BREG1003 used for block-item routes")
	fallbackIn := flag.String("fallback-in", "", "existing CVFB1001 inventory to filter")
	fallbackOut := flag.String("fallback-out", "", "path to write the filtered CVFB1001 inventory")
	fallbackBREG := flag.String("fallback-breg", "", "projected BREG1003 used to select fallback exclusions")
	fallbackRekeyIn := flag.String("fallback-rekey-in", "", "existing protocol-1001-keyed CVFB1001 inventory to rekey")
	rekeyLegacyBREG := flag.String("legacy-breg", "", "checked-in protocol-1001 BREG1003 naming the rekey input identities")
	rekeyNewBREG := flag.String("new-breg", "", "checked-in protocol-2225 BREG1003 supplying the rekeyed network hashes")
	fallbackRekeyOut := flag.String("fallback-rekey-out", "", "path to write the rekeyed CVFB1001 inventory")
	fallbackRekeyManifest := flag.String("fallback-rekey-manifest", "", "optional path to write the rekeyed source manifest")
	refreshBindings := flag.Bool("refresh-bindings", false, "bind derived registries to the newly generated BREG")
	flag.Parse()
	if *factsOut != "" {
		if err := exportCompilerFacts(*factsOut); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	if *fallbackRekeyIn != "" || *rekeyLegacyBREG != "" || *rekeyNewBREG != "" || *fallbackRekeyOut != "" || *fallbackRekeyManifest != "" {
		if *fallbackRekeyIn == "" || *rekeyLegacyBREG == "" || *rekeyNewBREG == "" || *fallbackRekeyOut == "" ||
			*out != "" || *lightOut != "" || *lightBREG != "" ||
			*physicsOut != "" || *physicsSHAOut != "" || *physicsBREG != "" ||
			*physicsV2225Out != "" || *physicsV2225SHAOut != "" || *physicsV2225BREG != "" || *physicsV2225Manifest != "" ||
			*biomeOut != "" || *biomeCoverage != "" ||
			*biomeV2225Allowlist != "" || *biomeV2225Manifest != "" ||
			*pmmpRoot != "" || *prismarineRoot != "" || *coverageManifest != "" ||
			*blockItemOut != "" || *blockItemBREG != "" ||
			*blockV2225LegacyBREG != "" ||
			*blockV2225Allowlist != "" || *blockV2225Manifest != "" ||
			*fallbackIn != "" || *fallbackOut != "" || *fallbackBREG != "" || *refreshBindings {
			fmt.Fprintln(os.Stderr, "registrygen: fallback rekey mode requires only -fallback-rekey-in, -legacy-breg, -new-breg, and -fallback-rekey-out")
			fmt.Fprintf(os.Stderr, "example: go run ./tools/registrygen -fallback-rekey-in crates/pack-compiler/data/vanilla-fallback-v1001.bin -legacy-breg crates/assets/data/block-registry-v1001.bin -new-breg crates/assets/data/block-registry-v2225.bin -fallback-rekey-out crates/assets/data/vanilla-fallback-v2225.bin [-fallback-rekey-manifest assets/vanilla-fallback-source-v2225.json]\n")
			os.Exit(2)
		}
		stats, err := writeRekeyedFallback(*fallbackRekeyIn, *rekeyLegacyBREG, *rekeyNewBREG, *fallbackRekeyOut, *fallbackRekeyManifest)
		if err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		if *fallbackRekeyManifest == "" {
			rekeyReportBytes, marshalErr := json.MarshalIndent(fallbackRekeyReport{
				InputEntries:     stats.InputEntries,
				ReservedExcluded: stats.ReservedExcluded,
				OutputEntries:    stats.OutputEntries,
				Output:           filepath.ToSlash(*fallbackRekeyOut),
			}, "", "  ")
			if marshalErr != nil {
				fmt.Fprintf(os.Stderr, "registrygen: encode rekey report: %v\n", marshalErr)
				os.Exit(1)
			}
			fmt.Println(string(rekeyReportBytes))
		}
		return
	}
	if *physicsV2225Out != "" || *physicsV2225BREG != "" || *physicsV2225SHAOut != "" || *physicsV2225Manifest != "" {
		if *physicsV2225Out == "" || *physicsV2225BREG == "" || *pmmpRoot != "" || *prismarineRoot != "" ||
			*out != "" || *lightOut != "" || *lightBREG != "" || *biomeOut != "" || *biomeCoverage != "" ||
			*biomeV2225Allowlist != "" || *biomeV2225Manifest != "" ||
			*coverageManifest != "" || *blockItemOut != "" || *blockItemBREG != "" ||
			*fallbackIn != "" || *fallbackOut != "" || *fallbackBREG != "" || *refreshBindings ||
			*physicsOut != "" || *physicsSHAOut != "" || *physicsBREG != "" ||
			*blockV2225LegacyBREG != "" ||
			*blockV2225Allowlist != "" || *blockV2225Manifest != "" {
			fmt.Fprintln(os.Stderr, "registrygen: v2225 physics mode requires only -physics-v2225-out and -physics-v2225-breg")
			fmt.Fprintf(os.Stderr, "example: go run ./tools/registrygen -physics-v2225-out %s -physics-v2225-breg %s [-physics-v2225-sha-out <path>] [-physics-v2225-manifest assets/block-projection-v2225.json]\n", v2225PhysicsOutputPath, v2225PhysicsBREGInputPath)
			os.Exit(2)
		}
		if err := writeV2225PhysicsProjection(*physicsV2225BREG, *physicsV2225Out, *physicsV2225SHAOut, *physicsV2225Manifest); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if *blockV2225LegacyBREG != "" || *blockV2225Allowlist != "" || *blockV2225Manifest != "" {
		if *out == "" || *blockV2225LegacyBREG == "" ||
			*blockV2225Allowlist == "" || *blockV2225Manifest == "" || *biomeOut != "" || *biomeCoverage != "" ||
			*biomeV2225Allowlist != "" || *biomeV2225Manifest != "" ||
			*lightBREG != "" || *physicsOut != "" || *physicsSHAOut != "" || *physicsBREG != "" || *pmmpRoot != "" ||
			*prismarineRoot != "" || *coverageManifest != "" || *blockItemOut != "" || *blockItemBREG != "" ||
			*fallbackIn != "" || *fallbackOut != "" || *fallbackBREG != "" || *refreshBindings {
			fmt.Fprintln(os.Stderr, "registrygen: v2225 block mode requires its legacy render registry, retail allowlist, output, and manifest flags")
			os.Exit(2)
		}
		if err := writeV2225BlockProjection(*blockV2225LegacyBREG, *blockV2225Allowlist, *out, *lightOut, *blockV2225Manifest); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if *biomeV2225Allowlist != "" || *biomeV2225Manifest != "" {
		if *biomeOut == "" || *biomeV2225Allowlist == "" || *biomeV2225Manifest == "" ||
			*biomeCoverage != "" || *out != "" || *lightOut != "" || *lightBREG != "" || *physicsOut != "" ||
			*physicsSHAOut != "" || *physicsBREG != "" || *pmmpRoot != "" || *prismarineRoot != "" ||
			*coverageManifest != "" || *blockItemOut != "" || *blockItemBREG != "" || *fallbackIn != "" ||
			*fallbackOut != "" || *fallbackBREG != "" || *refreshBindings {
			fmt.Fprintln(os.Stderr, "registrygen: v2225 biome mode requires only its allowlist, output, and manifest flags")
			os.Exit(2)
		}
		if err := writeV2225BiomeProjection(*biomeV2225Allowlist, *biomeOut, *biomeV2225Manifest); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if *biomeOut != "" && *out == "" && *lightOut == "" {
		if *biomeCoverage == "" || *lightBREG != "" || *physicsOut != "" || *physicsSHAOut != "" ||
			*physicsBREG != "" || *pmmpRoot != "" || *prismarineRoot != "" || *coverageManifest != "" ||
			*blockItemOut != "" || *blockItemBREG != "" || *fallbackIn != "" || *fallbackOut != "" ||
			*fallbackBREG != "" || *refreshBindings {
			fmt.Fprintln(os.Stderr, "registrygen: standalone biome mode requires only -biome-out and -biome-coverage")
			os.Exit(2)
		}
		if err := writeProjectedBiomeRegistry(*biomeOut, *biomeCoverage); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if (*biomeOut == "") != (*biomeCoverage == "") {
		fmt.Fprintln(os.Stderr, "registrygen: -biome-out and -biome-coverage must be supplied together")
		os.Exit(2)
	}
	if *fallbackIn != "" || *fallbackOut != "" || *fallbackBREG != "" {
		if *fallbackIn == "" || *fallbackOut == "" || *fallbackBREG == "" || *out != "" || *lightOut != "" || *blockItemOut != "" {
			fmt.Fprintln(os.Stderr, "registrygen: -fallback-in, -fallback-out, and -fallback-breg must be supplied together in standalone mode")
			os.Exit(2)
		}
		if err := writeFilteredFallback(*fallbackIn, *fallbackOut, *fallbackBREG); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if *blockItemOut != "" || *blockItemBREG != "" {
		if *blockItemOut == "" || *blockItemBREG == "" || *out != "" || *lightOut != "" {
			fmt.Fprintln(os.Stderr, "registrygen: -block-item-out and -block-item-breg must be supplied together in standalone mode")
			os.Exit(2)
		}
		if err := writeBlockItemRouteTable(*blockItemOut, *blockItemBREG); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		return
	}
	if *out == "" || *lightOut == "" || *lightBREG == "" {
		fmt.Fprintln(os.Stderr, "registrygen: -out, -light-out, and -light-breg are required")
		os.Exit(2)
	}
	physicsRequested := *physicsOut != "" || *physicsSHAOut != "" || *physicsBREG != ""
	if physicsRequested && (*physicsOut == "" || *physicsBREG == "") {
		fmt.Fprintln(os.Stderr, "registrygen: -physics-out and -physics-breg are required together")
		os.Exit(2)
	}

	var sourceRecords []Record
	var metadata RegistryMetadata
	var report GenerationReport
	var err error
	if *pmmpRoot == "" && *prismarineRoot == "" && *coverageManifest == "" {
		// The legacy source-free mode remains useful for focused Dragonfly
		// registry tests and biome-only generation. Release block registries use
		// the explicit four-source mode below.
		sourceRecords, err = collect(world.DefaultBlockRegistry)
	} else {
		if *pmmpRoot == "" || *prismarineRoot == "" || *coverageManifest == "" {
			fmt.Fprintln(os.Stderr, "registrygen: -pmmp, -prismarine, and -coverage must be supplied together")
			os.Exit(2)
		}
		sourceRecords, metadata, report, err = generateRegistry(*pmmpRoot, *prismarineRoot, *coverageManifest, world.DefaultBlockRegistry)
	}
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	records, err := projectRetailRegistry(sourceRecords)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	if err := validateRetailProjection(sourceRecords, records); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	metadata = metadataForRecords(records)
	if *coverageManifest != "" && (metadata.CanonicalNames != 1_188 || metadata.CanonicalStates != 16_913 || metadata.ValentineNames != 1_153 || metadata.ValentineStates != 15_845 || metadata.ValentineGapNames != 35 || metadata.ValentineGapStates != 1_068) {
		fmt.Fprintf(os.Stderr, "registrygen: projected metadata is %d/%d Valentine %d/%d gaps %d/%d\n", metadata.CanonicalNames, metadata.CanonicalStates, metadata.ValentineNames, metadata.ValentineStates, metadata.ValentineGapNames, metadata.ValentineGapStates)
		os.Exit(1)
	}
	encoded, err := encodeWithMetadata(metadata, records)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	var bindingBREG []byte
	if *refreshBindings {
		bindingBREG = encoded
	} else {
		bindingBREG, err = os.ReadFile(*lightBREG)
		if err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: read light-binding BREG: %v\n", err)
			os.Exit(1)
		}
	}
	if len(bindingBREG) > 128<<20 {
		fmt.Fprintln(os.Stderr, "registrygen: light-binding BREG exceeds 128 MiB")
		os.Exit(1)
	}
	if err := validateLightBindingBREG(bindingBREG, records); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	encodedLights, lightReport, err := encodeAuthoritativeLightRegistry(bindingBREG, sourceRecords, records, world.DefaultBlockRegistry, *pmmpRoot)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	report.LightMetadata = lightReport
	var encodedPhysics []byte
	if physicsRequested {
		var bindingPhysicsBREG []byte
		if *refreshBindings {
			bindingPhysicsBREG = encoded
		} else {
			bindingPhysicsBREG, err = os.ReadFile(*physicsBREG)
			if err != nil {
				fmt.Fprintf(os.Stderr, "registrygen: read physics-binding BREG: %v\n", err)
				os.Exit(1)
			}
		}
		if len(bindingPhysicsBREG) > 128<<20 {
			fmt.Fprintln(os.Stderr, "registrygen: physics-binding BREG exceeds 128 MiB")
			os.Exit(1)
		}
		if err := validatePhysicsBindingBREG(encoded, bindingPhysicsBREG, records); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		physicsSources, sourcesErr := loadPinnedPhysicsSources(*pmmpRoot, *prismarineRoot, world.DefaultBlockRegistry)
		if sourcesErr != nil {
			fmt.Fprintf(os.Stderr, "registrygen: read pinned physics sources: %v\n", sourcesErr)
			os.Exit(1)
		}
		physicsRecords, buildErr := buildPhysicsRecords(sourceRecords, physicsSources)
		if buildErr != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", buildErr)
			os.Exit(1)
		}
		if err := neutralizeReservedPhysics(physicsRecords); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
		encodedPhysics, err = encodePhysicsRegistry(bindingPhysicsBREG, physicsRecords, physicsRecordCount)
		if err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
			os.Exit(1)
		}
	}
	if err := os.MkdirAll(filepath.Dir(*out), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: create output directory: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*out, encoded, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write output: %v\n", err)
		os.Exit(1)
	}
	if err := os.MkdirAll(filepath.Dir(*lightOut), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: create light output directory: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*lightOut, encodedLights, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write light output: %v\n", err)
		os.Exit(1)
	}
	if physicsRequested {
		if err := os.MkdirAll(filepath.Dir(*physicsOut), 0o755); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: create physics output directory: %v\n", err)
			os.Exit(1)
		}
		if err := os.WriteFile(*physicsOut, encodedPhysics, 0o644); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: write physics output: %v\n", err)
			os.Exit(1)
		}
		physicsDigest := sha256.Sum256(encodedPhysics)
		shaOut := *physicsSHAOut
		if shaOut == "" {
			shaOut = strings.TrimSuffix(*physicsOut, filepath.Ext(*physicsOut)) + ".sha256"
		}
		if err := os.WriteFile(shaOut, []byte(fmt.Sprintf("%x\n", physicsDigest)), 0o644); err != nil {
			fmt.Fprintf(os.Stderr, "registrygen: write physics checksum: %v\n", err)
			os.Exit(1)
		}
	}
	digest := sha256.Sum256(encoded)
	shaPath := strings.TrimSuffix(*out, filepath.Ext(*out)) + ".sha256"
	if err := os.WriteFile(shaPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write checksum: %v\n", err)
		os.Exit(1)
	}
	lightDigest := sha256.Sum256(encodedLights)
	lightSHAPath := strings.TrimSuffix(*lightOut, filepath.Ext(*lightOut)) + ".sha256"
	if err := os.WriteFile(lightSHAPath, []byte(fmt.Sprintf("%x\n", lightDigest)), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write light checksum: %v\n", err)
		os.Exit(1)
	}
	if report.Protocol != 0 {
		reportBytes, marshalErr := json.MarshalIndent(report, "", "  ")
		if marshalErr != nil {
			fmt.Fprintf(os.Stderr, "registrygen: encode report: %v\n", marshalErr)
			os.Exit(1)
		}
		fmt.Println(string(reportBytes))
	}

	if *biomeOut == "" {
		return
	}
	if err := writeProjectedBiomeRegistry(*biomeOut, *biomeCoverage); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
}
