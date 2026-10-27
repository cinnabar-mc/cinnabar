package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/go-gl/mathgl/mgl32"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Material showcase: vanilla dyed and three-texture mobs beside two original entities whose
// materials come from the offered pack, one through an additive Bedrock material and one through a
// Cinnabar material binding with a glow mask.

const (
	materialShowcaseFirstID  = 9300000
	materialShowcaseAdditive = "cinnabar:material_additive"
	materialShowcaseGlow     = "cinnabar:material_glow"
	materialShowcaseTexture  = "textures/entity/material_showcase"
	materialShowcaseMask     = "textures/entity/material_showcase_glow"
	materialShowcaseControl  = "controller.render.cinnabar_material_showcase"
)

// materialShowcaseActors are spawned left to right, in this order.
var materialShowcaseActors = []string{
	"minecraft:sheep", "minecraft:wolf", "minecraft:llama", materialShowcaseAdditive, materialShowcaseGlow,
}

// materialShowcasePack offers the showcase entities, their materials and their bindings.
func materialShowcasePack() (*resource.Pack, error) {
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
	const name = "material-showcase-fixture"
	manifest, err := json.Marshal(resource.Manifest{
		FormatVersion: 2,
		Header: resource.Header{
			Name: "Material showcase fixture", Description: "Original entities for material captures",
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name)), Version: version, MinimumGameVersion: engine,
		},
		Modules: []resource.Module{{Type: "resources", Version: version,
			UUID: uuid.NewSHA1(uuid.Nil, []byte(name+"-resources")).String()}},
	})
	if err != nil {
		return nil, err
	}
	controller := fmt.Sprintf(`{"format_version":"1.8.0","render_controllers":{%q:{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}`, materialShowcaseControl)
	entity := func(identifier, material string) []byte {
		return fmt.Appendf(nil, `{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":%q,"materials":{"default":%q},"textures":{"default":%q},"geometry":{"default":%q},"render_controllers":[%q]}}}`,
			identifier, material, materialShowcaseTexture, actorBurstGeometryName, materialShowcaseControl)
	}
	materials := `{"materials":{"version":"1.0.0","cinnabar_additive:entity_alphatest":{"+states":["Blending"],"blendSrc":"One","blendDst":"One"}}}`
	bindings := fmt.Sprintf(`{"format_version":"1.0.0","materials":{"cinnabar:glow":{"base":"entity_alphatest","textures":{"glow_mask":%q},"parameters":{"strength":{"scalar":1.5}}}},"bindings":[{"entity":%q,"material":"cinnabar:glow"}]}`,
		materialShowcaseMask, materialShowcaseGlow)
	body, err := materialShowcaseImage(func(x, y int) color.NRGBA {
		return color.NRGBA{R: byte(40 + x), G: byte(60 + y), B: 140, A: 255}
	})
	if err != nil {
		return nil, err
	}
	// Bright bands every eight texels glow; the rest of the mask is transparent.
	mask, err := materialShowcaseImage(func(x, y int) color.NRGBA {
		if (x/4+y/4)%4 == 0 {
			return color.NRGBA{R: 255, G: 170, B: 40, A: 255}
		}
		return color.NRGBA{}
	})
	if err != nil {
		return nil, err
	}
	for _, file := range []struct {
		name string
		data []byte
	}{
		{"manifest.json", manifest},
		{"models/entity/actor_burst.geo.json", actorBurstGeometry(0)},
		{"render_controllers/material_showcase.json", []byte(controller)},
		{"materials/entity.material", []byte(materials)},
		{"entity/material_additive.entity.json", entity(materialShowcaseAdditive, "cinnabar_additive")},
		{"entity/material_glow.entity.json", entity(materialShowcaseGlow, "entity_alphatest")},
		{"cinnabar/material-bindings.json", []byte(bindings)},
		{materialShowcaseTexture + ".png", body},
		{materialShowcaseMask + ".png", mask},
	} {
		if err := add(file.name, file.data); err != nil {
			return nil, err
		}
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}

// materialShowcaseImage encodes one skin-sized PNG from `pixel`.
func materialShowcaseImage(pixel func(x, y int) color.NRGBA) ([]byte, error) {
	pixels := image.NewNRGBA(image.Rect(0, 0, actorBurstSkinSide, actorBurstSkinSide))
	for y := range actorBurstSkinSide {
		for x := range actorBurstSkinSide {
			pixels.SetNRGBA(x, y, pixel(x, y))
		}
	}
	var encoded bytes.Buffer
	err := png.Encode(&encoded, pixels)
	return encoded.Bytes(), err
}

// materialShowcasePackets spawns the showcase row four blocks ahead of `origin`.
func materialShowcasePackets(origin mgl32.Vec3) []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialShowcaseActors))
	for index, identifier := range materialShowcaseActors {
		id := int64(materialShowcaseFirstID + index)
		metadata := actorBurstMetadata()
		if identifier == "minecraft:sheep" {
			// A red fleece shows the dye colour mask.
			metadata[protocol.EntityDataKeyColorIndex] = byte(14)
		}
		packets = append(packets, &packet.AddActor{
			EntityUniqueID: id, EntityRuntimeID: uint64(id), EntityType: identifier,
			Position: origin.Add(mgl32.Vec3{float32(index*2) - float32(len(materialShowcaseActors)-1), 0, 4}),
			Yaw:      180, HeadYaw: 180, BodyYaw: 180,
			EntityMetadata: metadata,
		})
	}
	return packets
}

// materialShowcaseClear removes the previously spawned row.
func materialShowcaseClear() []packet.Packet {
	packets := make([]packet.Packet, 0, len(materialShowcaseActors))
	for index := range materialShowcaseActors {
		packets = append(packets, &packet.RemoveActor{EntityUniqueID: int64(materialShowcaseFirstID + index)})
	}
	return packets
}

// materialShowcaseListener adds /materialshowcase to opted-in local worlds.
func materialShowcaseListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &showcaseListener{Listener: listener}, nil
	}
}

type showcaseListener struct{ server.Listener }

// Accept tracks each connection's position for the showcase command.
func (l *showcaseListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &showcaseConn{Conn: conn}, nil
}

// Disconnect unwraps this fixture before delegating to the original listener.
func (l *showcaseListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*showcaseConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type showcaseConn struct {
	session.Conn
	position mgl32.Vec3
	spawned  bool
}

// ReadPacket respawns the showcase row ahead of the player on /materialshowcase.
func (c *showcaseConn) ReadPacket() (packet.Packet, error) {
	for {
		pk, err := c.Conn.ReadPacket()
		if err != nil {
			return pk, err
		}
		if movement, ok := pk.(*packet.PlayerAuthInput); ok {
			c.position = movement.Position
		}
		request, ok := pk.(*packet.CommandRequest)
		if !ok || strings.TrimSpace(strings.TrimPrefix(request.CommandLine, "/")) != "materialshowcase" {
			return pk, nil
		}
		var packets []packet.Packet
		if c.spawned {
			packets = materialShowcaseClear()
		}
		packets = append(packets, materialShowcasePackets(c.position)...)
		for _, fixture := range packets {
			if err := c.Conn.WritePacket(fixture); err != nil {
				return nil, err
			}
		}
		c.spawned = true
	}
}
