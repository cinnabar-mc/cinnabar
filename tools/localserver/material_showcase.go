package main

import (
	"archive/zip"
	"bytes"
	"embed"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"io/fs"
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
	materialShowcaseTexture  = "textures/entity/material_showcase.png"
	materialShowcaseMask     = "textures/entity/material_showcase_glow.png"
)

// materialShowcaseActors are spawned left to right, in this order.
var materialShowcaseActors = []string{
	"minecraft:sheep", "minecraft:wolf", "minecraft:llama", materialShowcaseAdditive, materialShowcaseGlow,
}

// materialShowcaseFiles holds the showcase pack's text files. Client tests compile these exact
// bytes, so the fixture and its regression tests cannot drift apart; its images are generated
// by materialShowcaseImages and pinned by fixtures/material_showcase.pixels.json.
//
//go:embed fixtures/material_showcase
var materialShowcaseFiles embed.FS

// materialShowcaseRoot is the embedded directory that holds the pack's root.
const materialShowcaseRoot = "fixtures/material_showcase"

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
	if err := add("manifest.json", manifest); err != nil {
		return nil, err
	}
	for _, image := range materialShowcaseImages() {
		encoded, err := image.encode()
		if err != nil {
			return nil, err
		}
		if err := add(image.name, encoded); err != nil {
			return nil, err
		}
	}
	err = fs.WalkDir(materialShowcaseFiles, materialShowcaseRoot, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		data, err := materialShowcaseFiles.ReadFile(path)
		if err != nil {
			return err
		}
		return add(strings.TrimPrefix(path, materialShowcaseRoot+"/"), data)
	})
	if err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return resource.ReadBytes(archive.Bytes())
}

// materialShowcaseImage is one generated pack texture.
type materialShowcaseImage struct {
	name  string
	pixel func(x, y int) color.NRGBA
}

// materialShowcaseImages are the pack's textures: a gradient body, so lighting and fog read
// against varying texels, and a glow mask whose bright bands glow while the rest is transparent.
func materialShowcaseImages() []materialShowcaseImage {
	return []materialShowcaseImage{
		{materialShowcaseTexture, func(x, y int) color.NRGBA {
			return color.NRGBA{R: byte(40 + x), G: byte(60 + y), B: 140, A: 255}
		}},
		{materialShowcaseMask, func(x, y int) color.NRGBA {
			if (x/4+y/4)%4 == 0 {
				return color.NRGBA{R: 255, G: 170, B: 40, A: 255}
			}
			return color.NRGBA{}
		}},
	}
}

// rgba is the image's skin-sized pixels, row by row.
func (i materialShowcaseImage) rgba() *image.NRGBA {
	pixels := image.NewNRGBA(image.Rect(0, 0, actorBurstSkinSide, actorBurstSkinSide))
	for y := range actorBurstSkinSide {
		for x := range actorBurstSkinSide {
			pixels.SetNRGBA(x, y, i.pixel(x, y))
		}
	}
	return pixels
}

// encode is the image as a PNG.
func (i materialShowcaseImage) encode() ([]byte, error) {
	var encoded bytes.Buffer
	err := png.Encode(&encoded, i.rgba())
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
