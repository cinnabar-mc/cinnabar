package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"testing"
)

func TestMaterialFxPackDeclaresEachShowcaseEntity(t *testing.T) {
	pack, err := materialFxPack()
	if err != nil {
		t.Fatal(err)
	}
	for _, entity := range materialFxEntities {
		short := strings.TrimPrefix(entity.id, materialFxID+":")
		data, err := pack.ReadFile("entity/materialfx_" + short + ".entity.json")
		if err != nil || !json.Valid(data) || !strings.Contains(string(data), entity.id) {
			t.Fatalf("%s: %v %s", entity.id, err, data)
		}
		if _, err := pack.ReadFile(materialFxTexture + short + ".png"); err != nil {
			t.Fatalf("%s skin: %v", entity.id, err)
		}
	}
	if got := len(materialFxActors(showcaseViewpoint{})); got != len(materialFxEntities) {
		t.Fatalf("%d actors", got)
	}
}

func TestMaterialFxCommandsBecomeControlRecords(t *testing.T) {
	record := func(command materialFxCommand) (uint16, int64) {
		if len(command.controls) != 1 {
			t.Fatalf("controls %+v", command)
		}
		return *command.controls[0][0].Choice, *command.controls[0][1].Integer
	}
	for _, test := range []struct {
		args  string
		op    uint16
		value int64
	}{
		{"revoke", materialFxUnbind, 0},
		{"restore", materialFxBind, 0},
		{"heartbeat 40", materialFxHeartbeat, 400},
		{"heartbeat off", materialFxHeartbeat, -1},
		{"cut 40", materialFxCut, 400},
		{"cut sweep", materialFxCut, -1},
	} {
		if op, value := record(parseMaterialFx(strings.Fields(test.args))); op != test.op || value != test.value {
			t.Errorf("%q: %d %d", test.args, op, value)
		}
	}
	for args, ok := range map[string]func(materialFxCommand) bool{
		"":        func(c materialFxCommand) bool { return c.spawn },
		"spawn":   func(c materialFxCommand) bool { return c.spawn },
		"boom":    func(c materialFxCommand) bool { return c.boom },
		"thermal": func(c materialFxCommand) bool { return c.thermal },
		"corrupt": func(c materialFxCommand) bool { return c.corrupt },
		"cleanse": func(c materialFxCommand) bool { return c.cleanse },
	} {
		if !ok(parseMaterialFx(strings.Fields(args))) {
			t.Errorf("%q", args)
		}
	}
	for _, bad := range []string{"heartbeat 101", "cut x", "dance", "boom now"} {
		if parseMaterialFx(strings.Fields(bad)).usage == "" {
			t.Errorf("%q is refused", bad)
		}
	}
	if got := len(materialFxRecord(materialFxBoom, 1, 2, 3)); got != 5 {
		t.Fatalf("a record carries an operation and four arguments: %d", got)
	}
}

// The control channel's schema and operation count match the Experience's declaration.
func TestMaterialFxRecordsMatchTheDeclaredChannel(t *testing.T) {
	manifest, err := os.ReadFile("../../examples/experiences/material-fx/experience.toml")
	if err != nil {
		t.Fatal(err)
	}
	text := string(manifest)
	if !strings.Contains(text, fmt.Sprintf("schema = %d\n", materialFxSchema)) {
		t.Errorf("experience.toml does not declare schema %d", materialFxSchema)
	}
	if !strings.Contains(text, fmt.Sprintf(`{ type = "choice", variants = %d }`, materialFxPortalDistance+1)) {
		t.Errorf("experience.toml does not declare %d operations", materialFxPortalDistance+1)
	}
}

// The gallery holds every piece: the arena floor, the 27-block black hole, the wall's hollow
// blocks, runes, painting, portal and screen, and the exhibits.
func TestMaterialFxGalleryHoldsEveryPiece(t *testing.T) {
	counts := map[string]int{}
	for _, name := range materialFxBlocks(showcaseViewpoint{}) {
		counts[strings.TrimPrefix(name, materialFxID+":")]++
	}
	for name, want := range map[string]int{
		"arena": 63, "blackhole": 27, "bottle": 1, "snowglobe": 1, "lavalamp": 1, "glintore": 4,
		"pulseore": 4, "aquarium": 1, "mineshaft": 1, "rune": 2, "canvas": 4, "vortex": 6, "screen": 12,
		"wallstone": 17*5 - 1 - 1 - 2 - 4 - 6 - 12,
	} {
		if counts[name] != want {
			t.Errorf("%s: %d blocks, want %d", name, counts[name], want)
		}
	}
	pieces := materialFxPieces(showcaseViewpoint{})
	if pieces[materialFxPortalPiece][1] != 1.5 {
		t.Errorf("the portal's centre stands in its middle row: %v", pieces[materialFxPortalPiece])
	}
}
