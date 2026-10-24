package main

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/go-gl/mathgl/mgl32"
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
	if got := len(materialFxActors(mgl32.Vec3{})); got != len(materialFxEntities) {
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
		{"", materialFxBind, 0},
		{"revoke", materialFxUnbind, 0},
		{"restore", materialFxBind, 0},
		{"status burning", materialFxStatus, 2},
		{"cut 40", materialFxCut, 400},
		{"cut sweep", materialFxCut, -1},
	} {
		if op, value := record(parseMaterialFx(strings.Fields(test.args))); op != test.op || value != test.value {
			t.Errorf("%q: %d %d", test.args, op, value)
		}
	}
	if !parseMaterialFx([]string{"spawn"}).spawn || !parseMaterialFx([]string{"flash"}).flash {
		t.Fatal("spawn and flash")
	}
	for _, bad := range []string{"status sparkly", "cut 101", "cut x", "dance"} {
		if parseMaterialFx(strings.Fields(bad)).usage == "" {
			t.Errorf("%q is refused", bad)
		}
	}
	if steps := materialFxFlashSteps; steps[0] != 1000 || steps[len(steps)-1] != 0 {
		t.Fatalf("a flash starts full and ends off: %v", steps)
	}
	blocks := materialFxBlocks(cube.Pos{})
	energy, portal := 0, 0
	for _, name := range blocks {
		switch name {
		case materialFxEnergy:
			energy++
		case materialFxPortal:
			portal++
		}
	}
	if energy != 6 || portal != 6 {
		t.Fatalf("%d energy and %d portal blocks", energy, portal)
	}
}
