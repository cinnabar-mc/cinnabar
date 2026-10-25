package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image/color"
	"math"
	"strings"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// World material showcase: the `materialfx` Experience's entities and blocks, which its client
// part restyles with programmable world materials (examples/experiences/material-fx). This
// fixture offers the entities' resource pack and adds /materialfx; the Experience artifact
// registers the blocks and the client part bundle draws them (see world-materials.md).

const (
	materialFxID        = "materialfx"
	materialFxChannel   = "materialfx.control"
	materialFxFirstID   = 9400000
	materialFxTexture   = "textures/entity/materialfx_"
	materialFxControl   = "controller.render.materialfx"
	materialFxEnergy    = "materialfx:energy"
	materialFxPortal    = "materialfx:portal"
	materialFxFlashStep = 90 * time.Millisecond
)

// The client part's control operations, as its experience.toml declares them.
const (
	materialFxBind uint16 = iota
	materialFxUnbind
	materialFxFlash
	materialFxStatus
	materialFxCut
)

// materialFxEntity is one showcase entity and its generated skin.
type materialFxEntity struct {
	id    string
	pixel func(x, y int) color.NRGBA
}

// materialFxEntities are spawned left to right: the dissolving wraith, the hologram and the
// sentinel whose status tint and damage flash the commands drive.
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
	{"materialfx:sentinel", func(x, y int) color.NRGBA {
		// Grey armour plates with rivets.
		if x%16 == 3 && y%16 == 3 {
			return color.NRGBA{R: 220, G: 220, B: 210, A: 255}
		}
		level := uint8(110 + (x/4+y/4)%2*25)
		return color.NRGBA{R: level, G: level, B: level + 8, A: 255}
	}},
}

// materialFxStatuses are the /materialfx status names, by the client part's status value.
var materialFxStatuses = []string{"none", "poison", "burning", "frozen"}

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

// materialFxCommand is one parsed /materialfx command: the actor and block packets or block
// placements it needs and the control records it sends the client part.
type materialFxCommand struct {
	spawn    bool
	controls [][]experience.Scalar
	// flash fades a damage flash over several control records.
	flash bool
	usage string
}

// materialFxRecord builds one control record.
func materialFxRecord(op uint16, value int64) []experience.Scalar {
	return []experience.Scalar{{Choice: &op}, {Integer: &value}}
}

// parseMaterialFx reads the arguments of /materialfx.
func parseMaterialFx(args []string) materialFxCommand {
	usage := "usage: /materialfx [spawn|revoke|restore|flash|status <none|poison|burning|frozen>|cut <0-100|sweep>]"
	switch {
	case len(args) == 0 || args[0] == "spawn":
		return materialFxCommand{spawn: true, controls: [][]experience.Scalar{materialFxRecord(materialFxBind, 0)}}
	case args[0] == "revoke" && len(args) == 1:
		return materialFxCommand{controls: [][]experience.Scalar{materialFxRecord(materialFxUnbind, 0)}}
	case args[0] == "restore" && len(args) == 1:
		return materialFxCommand{controls: [][]experience.Scalar{materialFxRecord(materialFxBind, 0)}}
	case args[0] == "flash" && len(args) == 1:
		return materialFxCommand{flash: true}
	case args[0] == "status" && len(args) == 2:
		for value, name := range materialFxStatuses {
			if name == args[1] {
				return materialFxCommand{controls: [][]experience.Scalar{materialFxRecord(materialFxStatus, int64(value))}}
			}
		}
	case args[0] == "cut" && len(args) == 2:
		if args[1] == "sweep" {
			return materialFxCommand{controls: [][]experience.Scalar{materialFxRecord(materialFxCut, -1)}}
		}
		var percent int64
		if _, err := fmt.Sscanf(args[1], "%d", &percent); err == nil && percent >= 0 && percent <= 100 {
			return materialFxCommand{controls: [][]experience.Scalar{materialFxRecord(materialFxCut, percent*10)}}
		}
	}
	return materialFxCommand{usage: usage}
}

// materialFxFlashSteps are a damage flash's intensities in thousandths, sent one step apart:
// full white at once, then fading out.
var materialFxFlashSteps = []int64{1000, 700, 450, 250, 100, 0}

// materialFxActors spawns the showcase entities at foot level in a row four blocks in front of
// the player, facing them.
func materialFxActors(view showcaseViewpoint) []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialFxEntities))
	for index, entity := range materialFxEntities {
		id := int64(materialFxFirstID + index)
		packets = append(packets, &packet.AddActor{
			EntityUniqueID: id, EntityRuntimeID: uint64(id), EntityType: entity.id,
			Position: view.at(float32(index*2-(len(materialFxEntities)-1)), 4),
			Yaw:      view.facingBack(), HeadYaw: view.facingBack(), BodyYaw: view.facingBack(),
			EntityMetadata: actorBurstMetadata(),
		})
	}
	return packets
}

// materialFxClear removes the previously spawned entities.
func materialFxClear() []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialFxEntities))
	for index := range materialFxEntities {
		packets = append(packets, &packet.RemoveActor{EntityUniqueID: int64(materialFxFirstID + index)})
	}
	return packets
}

// materialFxBlocks are the showcase's block placements in front of the player: a 3×2 wall of
// energy conduits on their right and a 2×3 star portal on their left, six blocks ahead,
// standing on the player's ground level.
func materialFxBlocks(view showcaseViewpoint) map[cube.Pos]string {
	placed := map[cube.Pos]string{}
	for side := -4; side <= -2; side++ {
		for up := 1; up <= 2; up++ {
			placed[view.block(side, up, 6)] = materialFxEnergy
		}
	}
	for side := 2; side <= 3; side++ {
		for up := 1; up <= 3; up++ {
			placed[view.block(side, up, 6)] = materialFxPortal
		}
	}
	return placed
}

// materialFxListener adds /materialfx to opted-in local worlds.
func materialFxListener(inner func(server.Config) (server.Listener, error), players func(uuid.UUID) (*world.EntityHandle, bool), ext *experienceSender) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &materialFxPacketListener{Listener: listener, players: players, ext: ext}, nil
	}
}

// experienceSender is the client part channel the commands drive; nil without -extension flags.
type experienceSender struct {
	send func(player uuid.UUID, exp, channel string, schema uint16, payload []experience.Scalar) bool
}

type materialFxPacketListener struct {
	server.Listener
	players func(uuid.UUID) (*world.EntityHandle, bool)
	ext     *experienceSender
}

// Accept tracks each connection's player and position for the showcase command.
func (l *materialFxPacketListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	id, _ := uuid.Parse(conn.IdentityData().Identity)
	return &materialFxConn{Conn: conn, players: l.players, ext: l.ext, id: id}, nil
}

// Disconnect unwraps this fixture before delegating to the original listener.
func (l *materialFxPacketListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*materialFxConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type materialFxConn struct {
	session.Conn
	players func(uuid.UUID) (*world.EntityHandle, bool)
	ext     *experienceSender
	id      uuid.UUID
	view    showcaseViewpoint
	spawned bool
}

// ReadPacket runs /materialfx: it spawns the showcase entities, places the blocks in the world
// and sends the client part its control records.
func (c *materialFxConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		if movement, ok := pk.(*packet.PlayerAuthInput); ok {
			c.view = viewpointOf(movement)
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok {
			return pk, nil
		}
		fields := strings.Fields(strings.TrimPrefix(strings.TrimSpace(request.CommandLine), "/"))
		if len(fields) == 0 || fields[0] != "materialfx" {
			return pk, nil
		}
		if err := c.run(parseMaterialFx(fields[1:])); err != nil {
			return nil, err
		}
	}
}

// run carries out one parsed command on this connection.
func (c *materialFxConn) run(command materialFxCommand) error {
	if command.usage != "" {
		c.tell(command.usage)
		return nil
	}
	if command.spawn {
		var packets []packet.Packet
		if c.spawned {
			packets = materialFxClear()
		}
		for _, fixture := range append(packets, materialFxActors(c.view)...) {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return err
			}
		}
		c.spawned = true
		c.placeBlocks()
	}
	send := func(record []experience.Scalar) {
		if c.ext == nil || !c.ext.send(c.id, materialFxID, materialFxChannel, 1, record) {
			c.tell("materialfx: no active client part (run the client with CINNABAR_DEV_SERVER_EXPERIENCES=1)")
		}
	}
	for _, record := range command.controls {
		send(record)
	}
	if command.flash {
		for step, value := range materialFxFlashSteps {
			record := materialFxRecord(materialFxFlash, value)
			time.AfterFunc(time.Duration(step)*materialFxFlashStep, func() { send(record) })
		}
	}
	return nil
}

// do runs `f` on the connection's player in its world, if it is still there.
func (c *materialFxConn) do(f func(tx *world.Tx, p *player.Player)) {
	handle, ok := c.players(c.id)
	if !ok {
		return
	}
	handle.Do(func(tx *world.Tx, e world.Entity) {
		if p, ok := e.(*player.Player); ok {
			f(tx, p)
		}
	})
}

// tell shows `text` to the connection's player.
func (c *materialFxConn) tell(text string) {
	c.do(func(_ *world.Tx, p *player.Player) { p.Message(text) })
}

// placeBlocks places the Experience's blocks ahead of the player, when it registered them,
// and says so when it did not.
func (c *materialFxConn) placeBlocks() {
	c.do(func(tx *world.Tx, p *player.Player) {
		for pos, name := range materialFxBlocks(c.view) {
			b, ok := world.BlockByName(name, nil)
			if !ok {
				p.Message("materialfx: the Experience's blocks are not registered (start the server with -experiences)")
				return
			}
			tx.SetBlock(pos, b, nil)
		}
	})
}
