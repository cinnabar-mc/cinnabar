package main

import (
	"bufio"
	"bytes"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
)

const (
	maximumInputBytes = 16 << 20
	packHeader        = "Pack Stack - [00] Local item capacity probe (id: a0318bd0-ae1c-4c36-9008-074c5c5cb4df, version: 1.0.0) @ behavior_packs/cinnabar_capacity_probe"
)

//go:embed probe/main.js
var probeScript []byte

//go:embed probe/manifest.json
var probeManifest []byte

var itemIdentifier = regexp.MustCompile(`^minecraft:[a-z0-9_]+$`)

type probeExpectations struct {
	version  string
	rowCount int
}

type capacityRow struct {
	identifier string
	maxCount   uint8
}

type provenance struct {
	Schema          string             `json:"schema"`
	SchemaVersion   int                `json:"schema_version"`
	GameVersion     string             `json:"game_version"`
	ProtocolVersion int                `json:"protocol_version"`
	Source          provenanceSource   `json:"source"`
	RetailAllowlist provenanceArtifact `json:"retail_allowlist"`
	Output          provenanceArtifact `json:"output"`
}

type provenanceSource struct {
	PublicBinarySHA256  string `json:"public_binary_sha256"`
	ProbeLogSHA256      string `json:"probe_log_sha256"`
	ProbeScriptSHA256   string `json:"probe_script_sha256"`
	ProbeManifestSHA256 string `json:"probe_manifest_sha256"`
	ItemTypeCount       int    `json:"item_type_count"`
	Failures            int    `json:"failures"`
}

type provenanceArtifact struct {
	Entries int    `json:"entries"`
	SHA256  string `json:"sha256"`
}

func main() {
	probeLogPath := flag.String("probe-log", "", "path to the complete public server probe log")
	retailPath := flag.String("retail-items", "", "path to the retail item allowlist TSV")
	outPath := flag.String("out", "", "capacity TSV output path")
	measurementPath := flag.String("measurement", "", "path to reviewed measurement metadata")
	provenancePath := flag.String("provenance-out", "", "provenance JSON output path")
	flag.Parse()

	if err := run(*probeLogPath, *retailPath, *outPath, *provenancePath, *measurementPath); err != nil {
		fmt.Fprintln(os.Stderr, "itemcapacity:", err)
		os.Exit(1)
	}
}

func run(probeLogPath, retailPath, outPath, provenancePath, measurementPath string) error {
	if probeLogPath == "" || retailPath == "" || outPath == "" || provenancePath == "" || measurementPath == "" {
		return errors.New("-probe-log, -retail-items, -out, -provenance-out, and -measurement are required")
	}
	if outPath == provenancePath {
		return errors.New("capacity and provenance output paths must differ")
	}
	measurement, err := readMeasurement(measurementPath)
	if err != nil {
		return err
	}
	if sha256Hex(probeScript) != measurement.Source.ProbeScriptSHA256 || sha256Hex(probeManifest) != measurement.Source.ProbeManifestSHA256 {
		return errors.New("bundled probe does not match measurement metadata")
	}

	logBytes, err := readBounded(probeLogPath)
	if err != nil {
		return fmt.Errorf("read probe log: %w", err)
	}
	if got := sha256Hex(logBytes); got != measurement.Source.ProbeLogSHA256 {
		return fmt.Errorf("probe log hash %s does not match pinned hash", got)
	}
	retailBytes, err := readBounded(retailPath)
	if err != nil {
		return fmt.Errorf("read retail allowlist: %w", err)
	}
	if got := sha256Hex(retailBytes); got != measurement.RetailAllowlist.SHA256 {
		return fmt.Errorf("retail allowlist hash %s does not match pinned hash", got)
	}

	rows, err := parseProbeLog(logBytes, probeExpectations{version: measurement.GameVersion, rowCount: measurement.Source.ItemTypeCount})
	if err != nil {
		return err
	}
	allowlist, err := parseRetailItems(retailBytes)
	if err != nil {
		return err
	}
	if len(allowlist) != measurement.RetailAllowlist.Entries {
		return fmt.Errorf("retail allowlist contains %d rows, want %d", len(allowlist), measurement.RetailAllowlist.Entries)
	}
	projected, err := projectRetail(rows, allowlist)
	if err != nil {
		return err
	}
	table := encodeCapacityTable(projected)
	metadata := measurement
	metadata.Output = provenanceArtifact{Entries: len(projected), SHA256: sha256Hex(table)}

	metadataBytes, err := encodeProvenance(metadata)
	if err != nil {
		return err
	}
	if err := writeFileAtomic(outPath, table); err != nil {
		return fmt.Errorf("write capacity table: %w", err)
	}
	if err := writeFileAtomic(provenancePath, metadataBytes); err != nil {
		return fmt.Errorf("write provenance: %w", err)
	}
	return nil
}

// readMeasurement validates the caller's recorded inputs before reading a raw measurement.
func readMeasurement(path string) (provenance, error) {
	data, err := readBounded(path)
	if err != nil {
		return provenance{}, err
	}
	var measurement provenance
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&measurement); err != nil {
		return provenance{}, err
	}
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		return provenance{}, errors.New("measurement metadata has trailing JSON")
	}
	if measurement.Schema != "cinnabar-item-capacity-provenance" || measurement.SchemaVersion != 1 ||
		measurement.ProtocolVersion != shared.ProtocolVersion || !strings.HasPrefix(measurement.GameVersion, shared.MinecraftVersion+".") ||
		measurement.Source.ItemTypeCount <= 0 || measurement.Source.Failures != 0 || measurement.RetailAllowlist.Entries <= 0 {
		return provenance{}, errors.New("measurement metadata does not match the target or contains failed rows")
	}
	for _, digest := range []string{measurement.Source.PublicBinarySHA256, measurement.Source.ProbeLogSHA256,
		measurement.Source.ProbeScriptSHA256, measurement.Source.ProbeManifestSHA256, measurement.RetailAllowlist.SHA256} {
		decoded, err := hex.DecodeString(digest)
		if err != nil || len(decoded) != sha256.Size || digest != strings.ToLower(digest) {
			return provenance{}, errors.New("invalid measurement digest")
		}
	}
	return measurement, nil
}

// encodeProvenance writes stable measurement metadata for the caller's evidence store.
func encodeProvenance(metadata provenance) ([]byte, error) {
	contents, err := json.MarshalIndent(metadata, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("encode provenance: %w", err)
	}
	return append(contents, '\n'), nil
}

func parseProbeLog(contents []byte, expected probeExpectations) (map[string]uint8, error) {
	rows := make(map[string]uint8, expected.rowCount)
	versionMarkers := 0
	packMarkers := 0
	doneMarkers := 0
	doneTotal := 0
	doneFailures := 0
	scanner := bufio.NewScanner(bytes.NewReader(contents))
	scanner.Buffer(make([]byte, 4096), 1<<20)
	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		if strings.Contains(line, " INFO] Version: ") {
			versionMarkers++
			if !strings.HasSuffix(line, "Version: "+expected.version) {
				return nil, fmt.Errorf("unexpected server version header")
			}
		}
		if strings.Contains(line, "Pack Stack - [00]") {
			if !strings.Contains(line, packHeader) {
				return nil, fmt.Errorf("unexpected probe pack header")
			}
			packMarkers++
		}
		if strings.Contains(line, "CINNABAR_CAPACITY_ERROR ") {
			return nil, errors.New("probe log contains a capacity error")
		}
		if payload, ok := markerPayload(line, "CINNABAR_CAPACITY_ROW "); ok {
			var fields []json.RawMessage
			if err := json.Unmarshal([]byte(payload), &fields); err != nil || len(fields) != 3 {
				return nil, errors.New("invalid capacity row")
			}
			var identifier string
			var maxCount int
			var stackable bool
			if json.Unmarshal(fields[0], &identifier) != nil || json.Unmarshal(fields[1], &maxCount) != nil || json.Unmarshal(fields[2], &stackable) != nil {
				return nil, errors.New("invalid capacity row fields")
			}
			if !itemIdentifier.MatchString(identifier) || maxCount < 1 || maxCount > 255 {
				return nil, errors.New("invalid capacity row value")
			}
			if stackable != (maxCount > 1) {
				return nil, errors.New("capacity row stackability disagrees with max count")
			}
			if _, exists := rows[identifier]; exists {
				return nil, errors.New("duplicate capacity row")
			}
			rows[identifier] = uint8(maxCount)
		}
		if payload, ok := markerPayload(line, "CINNABAR_CAPACITY_DONE "); ok {
			doneMarkers++
			var done struct {
				Total    int `json:"total"`
				Failures int `json:"failures"`
			}
			decoder := json.NewDecoder(strings.NewReader(payload))
			decoder.DisallowUnknownFields()
			if err := decoder.Decode(&done); err != nil {
				return nil, errors.New("invalid capacity completion marker")
			}
			doneTotal, doneFailures = done.Total, done.Failures
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("scan probe log: %w", err)
	}
	if versionMarkers != 1 || packMarkers != 1 {
		return nil, errors.New("probe log does not contain exactly one expected version and pack header")
	}
	if doneMarkers != 1 || doneTotal != expected.rowCount || doneFailures != 0 {
		return nil, errors.New("probe completion marker is missing or inconsistent")
	}
	if len(rows) != expected.rowCount {
		return nil, fmt.Errorf("probe log contains %d unique rows, want %d", len(rows), expected.rowCount)
	}
	return rows, nil
}

func markerPayload(line, marker string) (string, bool) {
	index := strings.Index(line, marker)
	if index < 0 {
		return "", false
	}
	return line[index+len(marker):], true
}

func parseRetailItems(contents []byte) ([]string, error) {
	var identifiers []string
	seenIDs := make(map[int]struct{})
	seenNames := make(map[string]struct{})
	scanner := bufio.NewScanner(bytes.NewReader(contents))
	for scanner.Scan() {
		fields := strings.Split(strings.TrimSuffix(scanner.Text(), "\r"), "\t")
		if len(fields) != 2 {
			return nil, errors.New("invalid retail item row")
		}
		id, err := strconv.Atoi(fields[0])
		if err != nil || !itemIdentifier.MatchString(fields[1]) {
			return nil, errors.New("invalid retail item row value")
		}
		if _, exists := seenIDs[id]; exists {
			return nil, errors.New("duplicate retail runtime ID")
		}
		if _, exists := seenNames[fields[1]]; exists {
			return nil, errors.New("duplicate retail item identifier")
		}
		seenIDs[id] = struct{}{}
		seenNames[fields[1]] = struct{}{}
		identifiers = append(identifiers, fields[1])
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("scan retail items: %w", err)
	}
	if len(identifiers) == 0 {
		return nil, errors.New("retail item allowlist is empty")
	}
	return identifiers, nil
}

func projectRetail(rows map[string]uint8, allowlist []string) ([]capacityRow, error) {
	projected := make([]capacityRow, 0, len(allowlist))
	for _, identifier := range allowlist {
		maxCount, ok := rows[identifier]
		if !ok {
			return nil, fmt.Errorf("retail identifier %s is missing from probe", identifier)
		}
		projected = append(projected, capacityRow{identifier: identifier, maxCount: maxCount})
	}
	sort.Slice(projected, func(i, j int) bool { return projected[i].identifier < projected[j].identifier })
	return projected, nil
}

func encodeCapacityTable(rows []capacityRow) []byte {
	var output bytes.Buffer
	for _, row := range rows {
		fmt.Fprintf(&output, "%s\t%d\n", row.identifier, row.maxCount)
	}
	return output.Bytes()
}

func readBounded(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	contents, err := io.ReadAll(io.LimitReader(file, maximumInputBytes+1))
	if err != nil {
		return nil, err
	}
	if len(contents) > maximumInputBytes {
		return nil, fmt.Errorf("input exceeds %d bytes", maximumInputBytes)
	}
	return contents, nil
}

func writeFileAtomic(path string, contents []byte) error {
	directory := filepath.Dir(path)
	temporary, err := os.CreateTemp(directory, ".itemcapacity-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer os.Remove(temporaryPath)
	if _, err := temporary.Write(contents); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Chmod(temporaryPath, 0o644); err != nil {
		return err
	}
	return os.Rename(temporaryPath, path)
}

func sha256Hex(contents []byte) string {
	hash := sha256.Sum256(contents)
	return hex.EncodeToString(hash[:])
}
