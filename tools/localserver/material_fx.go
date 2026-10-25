package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image/color"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// World material showcase: the `materialfx` Experience's entities and blocks, which its client
// part restyles with programmable world materials (examples/experiences/material-fx). This
// fixture offers the entities' resource pack and adds /materialfx; the Experience artifact
// registers the blocks and the client part bundle draws them (see world-materials.md).

const (
	materialFxID      = "materialfx"
	materialFxChannel = "materialfx.control"
	// materialFxSchema is the control channel's schema, as experience.toml declares it.
	materialFxSchema  = 2
	materialFxFirstID = 9400000
	materialFxTexture = "textures/entity/materialfx_"
	materialFxControl = "controller.render.materialfx"
	// materialFxClockMs is the session clock's wrap: world materials animate on the wall clock
	// modulo 4096 s, so the server times effects on the same clock.
	materialFxClockMs = 4096000
	// materialFxArgLimit bounds every integer argument of a control record, as experience.toml
	// declares it: positions in thousandths reach ±100,000 blocks.
	materialFxArgLimit = 100_000_000
)

// The client part's control operations, numbered as experience.toml's `# op` lines number them
// (TestMaterialFxRecordsMatchTheDeclaredChannel checks each by name).
const (
	materialFxBind uint16 = iota
	materialFxUnbind
	materialFxCut
	materialFxBoom
	materialFxHeartbeat
	materialFxHit
	materialFxThermal
	materialFxCorrupt
	materialFxLayout
	materialFxFacing
	materialFxRadar
	materialFxPortalDistance
)

// The layout pieces whose centres the client part needs, by the layout operation's piece index.
const (
	materialFxArenaPiece = iota
	materialFxBlackHolePiece
	materialFxPaintingPiece
	materialFxScreenPiece
	materialFxPortalPiece
)

// materialFxEntity is one showcase entity and its generated skin.
type materialFxEntity struct {
	id    string
	pixel func(x, y int) color.NRGBA
}

// materialFxMetal is the liquid metal mob, the one hits are aimed at.
const materialFxMetal = "materialfx:metal"

// materialFxEntities are spawned left to right: the dissolving wraith, the hologram, the void
// walker, the lenticular mob and the liquid metal target.
var materialFxEntities = []materialFxEntity{
	{"materialfx:wraith", func(x, y int) color.NRGBA {
		// Pale robe cloth with darker folds.
		fold := uint8(20 * math.Abs(math.Sin(float64(x)*0.7+float64(y)*0.15)))
		return color.NRGBA{R: 170 - fold, G: 180 - fold, B: 200 - fold, A: 255}
	}},
	{"materialfx:hologram", func(x, y int) color.NRGBA {
		// A bright figure with a grid, which the hologram turns into its own light.
		if x%8 == 0 || y%8 == 0 {
			return color.NRGBA{R: 240, G: 250, B: 255, A: 255}
		}
		return color.NRGBA{R: 120, G: 140, B: 160, A: 255}
	}},
	{"materialfx:voidwalker", func(x, y int) color.NRGBA {
		// A dark figure; its world material shows space through it.
		return color.NRGBA{R: 30, G: 20, B: 45, A: 255}
	}},
	{"materialfx:lenticular", func(x, y int) color.NRGBA {
		// A plain figure; its world material picks one of three faces by angle.
		return color.NRGBA{R: 200, G: 170, B: 120, A: 255}
	}},
	{materialFxMetal, func(x, y int) color.NRGBA {
		// Brushed steel.
		level := uint8(150 + (x+y*3)%7*6)
		return color.NRGBA{R: level, G: level, B: level + 10, A: 255}
	}},
}

// materialFxPack offers the showcase entities. Their materials are Bedrock's entity_alphatest
// until the client part binds its world materials.
func materialFxPack() (*resource.Pack, error) {
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	add := func(name string, data []byte) error {
		file, err := writer.Create(name)
		if err != nil {
			return err
		}
		_, err = file.Write(data)
		return err
	}
	var engine resource.Version
	if _, err := fmt.Sscanf(protocol.CurrentVersion, "%d.%d.%d", &engine[0], &engine[1], &engine[2]); err != nil {
		return nil, err
	}
	version := resource.Version{1, 0, 0}
	const name = "material-fx-fixture"
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{
			Name: "World material showcase", Description: "Original entities for world material captures",
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name)), Version: version, MinimumGameVersion: engine,
		},
		Modules: []resource.Module{{Type: "resources", Version: version,
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name+"-resources")).String()}},
	})
	if err != nil {
		return nil, err
	}
	files := []struct {
		name string
		data []byte
	}{
		{"manifest.json", manifest},
		{"models/entity/actor_burst.geo.json", actorBurstGeometry(0)},
		{"render_controllers/materialfx.json", fmt.Appendf(nil,
			`{"format_version":"1.8.0","render_controllers":{%q:{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}`,
			materialFxControl)},
	}
	for _, entity := range materialFxEntities {
		short := strings.TrimPrefix(entity.id, materialFxID+":")
		skin, err := materialShowcaseImage{materialFxTexture + short + ".png", entity.pixel}.encode()
		if err != nil {
			return nil, err
		}
		files = append(files,
			struct {
				name string
				data []byte
			}{"entity/materialfx_" + short + ".entity.json", fmt.Appendf(nil,
				`{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":%q,"materials":{"default":"entity_alphatest"},"textures":{"default":%q},"geometry":{"default":%q},"render_controllers":[%q]}}}`,
				entity.id, materialFxTexture+short, actorBurstGeometryName, materialFxControl)},
			struct {
				name string
				data []byte
			}{materialFxTexture + short + ".png", skin},
		)
	}
	for _, file := range files {
		if err := add(file.name, file.data); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}

// materialFxCarried reports whether every argument of `record` lies within the control
// channel's ±materialFxArgLimit, so the client part can accept it.
func materialFxCarried(record []cinnabarext.Scalar) bool {
	return !slices.ContainsFunc(record, func(value cinnabarext.Scalar) bool {
		return value.Integer != nil && (*value.Integer < -materialFxArgLimit || *value.Integer > materialFxArgLimit)
	})
}

// materialFxRecord builds one control record of operation `op` with up to four arguments.
func materialFxRecord(op uint16, args ...int64) []cinnabarext.Scalar {
	record := []cinnabarext.Scalar{{Choice: &op}}
	for index := range 4 {
		var value int64
		if index < len(args) {
			value = args[index]
		}
		record = append(record, cinnabarext.Scalar{Integer: &value})
	}
	return record
}

// materialFxNowMs is the session clock now, in milliseconds.
func materialFxNowMs(now time.Time) int64 {
	return now.UnixMilli() % materialFxClockMs
}

// milli is a block coordinate in the control channel's thousandths.
func milli(value float32) int64 {
	return int64(math.Round(float64(value) * 1000))
}

// materialFxCommand is one parsed /materialfx command.
type materialFxCommand struct {
	usage string
	// spawn builds the gallery in front of the player.
	spawn bool
	// controls go to the client part as they are.
	controls [][]cinnabarext.Scalar
	// boom starts a shockwave at the arena's centre now.
	boom bool
	// thermal toggles thermal vision.
	thermal bool
	// corrupt starts the corruption at the wall's end now; cleanse stops it.
	corrupt, cleanse bool
}

// materialFxUsage lists the /materialfx commands.
const materialFxUsage = "usage: /materialfx [spawn|revoke|restore|boom|heartbeat <0-100|off>|thermal|corrupt|cleanse|cut <0-100|sweep>]"

// parseMaterialFx reads the arguments of /materialfx.
func parseMaterialFx(args []string) materialFxCommand {
	number := func(text string) (int64, bool) {
		var value int64
		_, err := fmt.Sscanf(text, "%d", &value)
		return value, err == nil && value >= 0 && value <= 100
	}
	switch {
	case len(args) == 0 || (args[0] == "spawn" && len(args) == 1):
		return materialFxCommand{spawn: true}
	case args[0] == "revoke" && len(args) == 1:
		return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxUnbind)}}
	case args[0] == "restore" && len(args) == 1:
		return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxBind)}}
	case args[0] == "boom" && len(args) == 1:
		return materialFxCommand{boom: true}
	case args[0] == "thermal" && len(args) == 1:
		return materialFxCommand{thermal: true}
	case args[0] == "corrupt" && len(args) == 1:
		return materialFxCommand{corrupt: true}
	case args[0] == "cleanse" && len(args) == 1:
		return materialFxCommand{cleanse: true}
	case args[0] == "heartbeat" && len(args) == 2:
		if args[1] == "off" {
			return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxHeartbeat, -1)}}
		}
		if health, ok := number(args[1]); ok {
			return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxHeartbeat, health*10)}}
		}
	case args[0] == "cut" && len(args) == 2:
		if args[1] == "sweep" {
			return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxCut, -1)}}
		}
		if percent, ok := number(args[1]); ok {
			return materialFxCommand{controls: [][]cinnabarext.Scalar{materialFxRecord(materialFxCut, percent*10)}}
		}
	}
	return materialFxCommand{usage: materialFxUsage}
}
