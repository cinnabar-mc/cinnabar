package main

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"regexp"
	"strconv"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
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
		{"cut 40", materialFxCut, 400},
		{"cut sweep", materialFxCut, -1},
		{"wind 30", materialFxWind, 300},
		{"heat 100", materialFxHeat, 1000},
		{"refract 0", materialFxRefract, 0},
		{"extinguish", materialFxFire, -1},
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
		"jelly":   func(c materialFxCommand) bool { return c.jelly },
		"outline": func(c materialFxCommand) bool { return c.outline },
		"bloom":   func(c materialFxCommand) bool { return c.bloom },
	} {
		if !ok(parseMaterialFx(strings.Fields(args))) {
			t.Errorf("%q", args)
		}
	}
	for args, health := range map[string]int64{"heartbeat 40": 400, "heartbeat off": -1} {
		if command := parseMaterialFx(strings.Fields(args)); command.heartbeat == nil || *command.heartbeat != health {
			t.Errorf("%q: %+v", args, command)
		}
	}
	for args, op := range map[string]uint16{"shatter": materialFxShatter, "pulse": materialFxPulse, "storm": materialFxStorm, "ignite": materialFxFire} {
		if command := parseMaterialFx(strings.Fields(args)); len(command.timed) != 1 || command.timed[0] != op {
			t.Errorf("%q: %+v", args, command)
		}
	}
	for _, bad := range []string{"heartbeat 101", "cut x", "dance", "boom now", "wind", "wind 101", "storm now"} {
		if parseMaterialFx(strings.Fields(bad)).usage == "" {
			t.Errorf("%q is refused", bad)
		}
	}
	if got := len(materialFxRecord(materialFxBoom, 1, 2, 3)); got != 5 {
		t.Fatalf("a record carries an operation and four arguments: %d", got)
	}
}

// The control channel's schema matches the Experience's declaration, and each operation's
// number matches the `# op <index> <name>:` line experience.toml numbers it with, which the
// client part's build script derives its own constants from.
func TestMaterialFxRecordsMatchTheDeclaredChannel(t *testing.T) {
	manifest, err := os.ReadFile("../../examples/experiences/material-fx/experience.toml")
	if err != nil {
		t.Fatal(err)
	}
	text := string(manifest)
	if !strings.Contains(text, fmt.Sprintf("schema = %d\n", materialFxSchema)) {
		t.Errorf("experience.toml does not declare schema %d", materialFxSchema)
	}
	operations := map[string]uint16{
		"bind": materialFxBind, "unbind": materialFxUnbind, "cut": materialFxCut, "boom": materialFxBoom,
		"heartbeat": materialFxHeartbeat, "hit": materialFxHit, "thermal": materialFxThermal,
		"corrupt": materialFxCorrupt, "layout": materialFxLayout, "facing": materialFxFacing,
		"radar": materialFxRadar, "portal_distance": materialFxPortalDistance,
		"wind": materialFxWind, "jelly_hit": materialFxJellyHit, "shatter": materialFxShatter, "heat": materialFxHeat, "refract": materialFxRefract, "pulse": materialFxPulse, "outline": materialFxOutline, "bloom": materialFxBloom, "storm": materialFxStorm, "fire": materialFxFire,
	}
	listed := regexp.MustCompile(`(?m)^# op (\d+) ([a-z_]+):`).FindAllStringSubmatch(text, -1)
	for index, line := range listed {
		number, name := line[1], line[2]
		op, ok := operations[name]
		if !ok || number != strconv.Itoa(index) || op != uint16(index) {
			t.Errorf("experience.toml numbers %s %s (line %d); the server sends it as %d (known %v)", name, number, index, op, ok)
		}
		delete(operations, name)
	}
	for name := range operations {
		t.Errorf("experience.toml does not number operation %s", name)
	}
	if !strings.Contains(text, fmt.Sprintf(`{ type = "choice", variants = %d }`, len(listed))) || len(listed) != int(materialFxFire)+1 {
		t.Errorf("experience.toml's choice does not admit exactly its %d operations", len(listed))
	}
	bound := fmt.Sprintf(`{ type = "integer", min = %d, max = %d }`, -materialFxArgLimit, materialFxArgLimit)
	if got := strings.Count(text, `type = "integer"`); got != 4 || strings.Count(text, bound) != got {
		t.Errorf("experience.toml's four integer arguments are not all bounded as %s", bound)
	}
}

// The gallery holds every piece: the arena floor, the 27-block black hole, the wall's hollow
// blocks, runes, painting, portal and screen, the exhibits, and the effects wing.
func TestMaterialFxGalleryHoldsEveryPiece(t *testing.T) {
	counts := map[string]int{}
	for _, name := range materialFxBlocks(showcaseViewpoint{}) {
		counts[strings.TrimPrefix(name, materialFxID+":")]++
	}
	for name, want := range map[string]int{
		"arena": 63, "blackhole": 27, "bottle": 1, "snowglobe": 1, "lavalamp": 1, "glintore": 4,
		"pulseore": 4, "aquarium": 1, "mineshaft": 1, "rune": 2, "canvas": 4, "vortex": 6, "screen": 12,
		"wallstone": 17*5 - 1 - 1 - 2 - 4 - 6 - 12 + 4, "haze": 2, "wavegrass": 35, "lens": 4,
		"forcefield": 9, "storm": 1, "flag": 6, "firewall": 12,
	} {
		if counts[name] != want {
			t.Errorf("%s: %d blocks, want %d", name, counts[name], want)
		}
	}
	pieces := materialFxPieces(showcaseViewpoint{})
	if pieces[materialFxPortalPiece][1] != 1.5 {
		t.Errorf("the portal's centre stands in its middle row: %v", pieces[materialFxPortalPiece])
	}
	if pieces[materialFxFlagPiece][1] != 3 || pieces[materialFxFirePiece][1] != 1.5 || pieces[materialFxGrassPiece][1] != 0 {
		t.Errorf("the flag, fire wall and grass top: %v %v %v", pieces[materialFxFlagPiece], pieces[materialFxFirePiece], pieces[materialFxGrassPiece])
	}
}

// No two showcase entities stand in the same place or inside a placed block.
func TestMaterialFxEntitiesStandApartAndInTheOpen(t *testing.T) {
	blocks := materialFxBlocks(showcaseViewpoint{})
	seen := map[[2]float32]string{}
	for _, entity := range materialFxEntities {
		place := [2]float32{entity.side, entity.ahead}
		if other, ok := seen[place]; ok {
			t.Errorf("%s stands where %s does", entity.id, other)
		}
		seen[place] = entity.id
		feet := materialFxActorAt(showcaseViewpoint{}, entity)
		for up := range 2 {
			pos := showcaseViewpoint{}.ground().Add(cube.Pos{int(math.Floor(float64(feet[0]))), 1 + up, int(math.Floor(float64(feet[2])))})
			if name, ok := blocks[pos]; ok {
				t.Errorf("%s stands inside %s", entity.id, name)
			}
		}
	}
}
