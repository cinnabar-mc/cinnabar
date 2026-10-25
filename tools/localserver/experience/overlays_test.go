package experience

import (
	"bytes"
	"errors"
	"flag"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

var updateGoFixtures = flag.Bool("update-go-fixtures", false, "rewrite testdata/go from the adapter")

// goFixtureDir holds what the adapter itself produces for the Rust client: crates/world decodes
// cell_overlays.nbt.
var goFixtureDir = filepath.Join("testdata", "go")

// plate is the probe's plate overlay in slot on face.
func plate(slot uint8, face Face) Overlay {
	return Overlay{Slot: slot, Face: face, Look: OverlayLook{Visual: &OverlayVisual{Kind: "probe:plate"}}}
}

// diamond is a diamond item overlay in slot on face, size pixels wide at x, y.
func diamond(slot uint8, face Face, size, x, y float32) Overlay {
	return Overlay{Slot: slot, Face: face, Look: OverlayLook{Item: &OverlayItem{ID: "minecraft:diamond", Size: size, X: x, Y: y}}}
}

// bus is the probe's overlay host in its default state.
func bus(t *testing.T) Block {
	t.Helper()
	b, ok := registered(t).Lookup("probe:bus")
	if !ok {
		t.Fatal("the probe registers no probe:bus")
	}
	return b
}

// The block entity of a bus carrying a plate on its north face in slot 2 and a diamond on its top
// in slot 5, as Dragonfly sends it with the bus's position, is checked in so crates/world can
// decode exactly what the adapter sends. Regenerate with
// `go test ./experience -run TestCellOverlaysFixtureIsCurrent -update-go-fixtures`.
func TestCellOverlaysFixtureIsCurrent(t *testing.T) {
	b := bus(t)
	list, err := b.t.overlays.check(Overlays{plate(2, FaceNorth), diamond(5, FaceUp, 6.4, 0, 0.8)})
	if err != nil {
		t.Fatal(err)
	}
	m := HostBlock{Block: b, overlays: list}.EncodeNBT()
	m["x"], m["y"], m["z"] = int32(0), int32(0), int32(0)
	var buf bytes.Buffer
	enc := nbt.NewEncoderWithEncoding(&buf, nbt.NetworkLittleEndian)
	enc.SortMaps = true
	if err := enc.Encode(m); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(goFixtureDir, "cell_overlays.nbt")
	if *updateGoFixtures {
		if err := os.MkdirAll(goFixtureDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, buf.Bytes(), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if got, err := os.ReadFile(path); err != nil || !bytes.Equal(got, buf.Bytes()) {
		t.Fatalf("%s is stale (%v); regenerate with `go test ./experience -run TestCellOverlaysFixtureIsCurrent -update-go-fixtures`", path, err)
	}
}

// A host's overlays survive its block entity's round trip, and a saved overlay that no longer
// checks is dropped on the way back while the rest stay.
func TestHostOverlaysRoundTripTheirBlockEntity(t *testing.T) {
	b := bus(t)
	overlays := Overlays{plate(0, FaceDown), plate(3, FaceSouth), diamond(7, FaceEast, 4, -2, 3)}
	list, err := b.t.overlays.check(overlays)
	if err != nil {
		t.Fatal(err)
	}
	saved := HostBlock{Block: b, overlays: list}.EncodeNBT()
	back := HostBlock{Block: b}.DecodeNBT(saved).(HostBlock)
	if got := back.overlays.list(); !overlaysEqual(got, overlays) {
		t.Fatalf("decoded %+v, want %+v", got, overlays)
	}
	if host, _ := saved["host"].(map[string]any); host["name"] != "probe:bus" {
		t.Fatalf("host %v, want probe:bus", saved["host"])
	}
	entries := slices.Clone(saved["overlays"].([]any))
	stale := map[string]any{"slot": uint8(5), "face": uint8(1), "block": map[string]any{"name": "probe:gone", "states": map[string]any{}}}
	entries = slices.Insert(entries, 2, any(stale))
	saved["overlays"] = entries
	back = HostBlock{Block: b}.DecodeNBT(saved).(HostBlock)
	if got := back.overlays.list(); !overlaysEqual(got, overlays) {
		t.Fatalf("with a stale overlay decoded %+v, want %+v", got, overlays)
	}
}

// overlaysEqual compares overlay lists by value.
func overlaysEqual(a, b Overlays) bool {
	return slices.EqualFunc(a, b, func(x, y Overlay) bool {
		if x.Slot != y.Slot || x.Face != y.Face || (x.Look.Visual == nil) != (y.Look.Visual == nil) {
			return false
		}
		if x.Look.Visual != nil {
			return x.Look.Visual.Kind == y.Look.Visual.Kind && len(x.Look.Visual.States) == len(y.Look.Visual.States)
		}
		return *x.Look.Item == *y.Look.Item
	})
}

// The adapter checks a host's overlays as the runtime does before it stages them, and a block
// that is no host carries none.
func TestOverlayListsAreChecked(t *testing.T) {
	b := bus(t)
	thirteen := make(Overlays, 0, maxCellOverlays+1)
	for slot := range uint8(maxCellOverlays + 1) {
		thirteen = append(thirteen, plate(slot, FaceNorth))
	}
	for _, c := range []struct {
		name     string
		overlays Overlays
	}{
		{"thirteen overlays", thirteen},
		{"a slot past 11", Overlays{plate(maxCellOverlays, FaceNorth)}},
		{"slots out of order", Overlays{plate(2, FaceNorth), plate(1, FaceSouth)}},
		{"a slot twice", Overlays{plate(1, FaceNorth), plate(1, FaceSouth)}},
		{"an unknown face", Overlays{plate(0, Face("inward"))}},
		{"another Experience's type", Overlays{{Slot: 0, Face: FaceUp, Look: OverlayLook{Visual: &OverlayVisual{Kind: "other:plate"}}}}},
		{"an unknown item", Overlays{{Slot: 0, Face: FaceUp, Look: OverlayLook{Item: &OverlayItem{ID: "probe:nothing", Size: 4}}}}},
		{"an empty item", Overlays{diamond(0, FaceUp, 0, 0, 0)}},
		{"an item wider than the face", Overlays{diamond(0, FaceUp, maxOverlayItemSize+1, 0, 0)}},
		{"an item off the face", Overlays{diamond(0, FaceUp, 4, maxOverlayItemOffset+1, 0)}},
		{"a look without a visual or item", Overlays{{Slot: 0, Face: FaceUp}}},
	} {
		if _, err := b.t.overlays.check(c.overlays); !errors.Is(err, errBadOverlay) {
			t.Errorf("%s: %v, want a bad overlay", c.name, err)
		}
	}
	if _, err := b.t.overlays.check(Overlays{plate(0, FaceUp), diamond(11, FaceDown, 16, 8, -8)}); err != nil {
		t.Errorf("a plate and an item at the bounds: %v", err)
	}
	counter, ok := registered(t).Lookup("probe:counter")
	if !ok {
		t.Fatal("the probe registers no probe:counter")
	}
	if _, err := counter.t.overlays.check(nil); !errors.Is(err, errBadOverlay) {
		t.Errorf("a block that is no host: %v, want a bad overlay", err)
	}
}

// Overlay types are hidden blocks: no Experience block of that id, so set-block cannot place one,
// and no item.
func TestOverlayTypesAreNoBlocksOrItems(t *testing.T) {
	r := registered(t)
	if _, ok := r.Lookup("probe:plate"); ok {
		t.Error("the overlay type probe:plate is an Experience block")
	}
	kind, ok := r.overlays["probe:plate"]
	if !ok {
		t.Fatal("probe:plate is not registered as an overlay type")
	}
	if _, ok := world.ItemByName("probe:plate", 0); ok {
		t.Error("the overlay type probe:plate has an item")
	}
	if !kind.overlay {
		t.Error("probe:plate is not marked as an overlay type")
	}
}

// A click's position reaches on-interact in pixels only when it lies within the cell.
func TestHitOfKeepsClicksWithinTheCell(t *testing.T) {
	at := func(x, y, z float64) *item.UseContext {
		return &item.UseContext{ClickPos: &mgl64.Vec3{x, y, z}}
	}
	for _, c := range []struct {
		ctx  *item.UseContext
		want *Pixel
	}{
		{nil, nil},
		{&item.UseContext{}, nil},
		{at(0.5, 1, 0), &Pixel{X: 8, Y: 16, Z: 0}},
		{at(0.25, 0.0625, 0.75), &Pixel{X: 4, Y: 1, Z: 12}},
		{at(1.01, 0.5, 0.5), nil},
		{at(0.5, -0.01, 0.5), nil},
	} {
		got := hitOf(c.ctx)
		if (got == nil) != (c.want == nil) || got != nil && *got != *c.want {
			t.Errorf("hitOf(%+v) = %+v, want %+v", c.ctx, got, c.want)
		}
	}
}

// Using the probe's bus puts a plate on the clicked face, and a click on that plate removes it,
// the runtime having picked it from the click position.
func TestBusPlatesComeAndGoByClick(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := BlockPos{X: 3, Y: 100, Z: 44}
	f.place("probe:bus", pos, nil)
	click := func(face cube.Face, at mgl64.Vec3) {
		f.do(func(tx *world.Tx) {
			p := f.player(tx)
			tx.Block(pos.cube()).(HostBlock).Activate(pos.cube(), face, tx, p, &item.UseContext{ClickPos: &at})
		})
	}
	overlays := func() Overlays {
		var got Overlays
		f.do(func(tx *world.Tx) { got = tx.Block(pos.cube()).(HostBlock).overlays.list() })
		return got
	}
	click(cube.FaceNorth, mgl64.Vec3{0.5, 0.5, 0})
	f.waitTells(5*time.Second, "plate 2 ok")
	if got := overlays(); !overlaysEqual(got, Overlays{plate(2, FaceNorth)}) {
		t.Fatalf("overlays after the first click %+v, want the north plate", got)
	}
	// Inside the plate, two pixels deep against the north face.
	click(cube.FaceNorth, mgl64.Vec3{0.5, 0.5, 0.1})
	f.waitTells(5*time.Second, "plate 2 ok", "target 2 ok")
	if got := overlays(); len(got) != 0 {
		t.Fatalf("overlays after clicking the plate %+v, want none", got)
	}
}
