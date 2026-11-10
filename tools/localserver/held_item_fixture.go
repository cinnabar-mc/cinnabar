package main

import (
	"context"
	"image"
	"image/color"
	"maps"
	"slices"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/item/category"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// renderOffsetsComponent is the item component the fixture adds to the registry entries it declares.
const renderOffsetsComponent = "minecraft:render_offsets"

// heldItemFixtureArt is an original pick with its head at the top right and a yellow grip at the
// bottom left, so a held item's size, lean and mirroring read at a glance.
var heldItemFixtureArt = [16]string{
	"................",
	"..........CCCC..",
	"............CCC.",
	".............CCR",
	"...........B.CCR",
	"..........B...CC",
	".........B....CC",
	"........B.....C.",
	".......B......C.",
	"......B.........",
	".....B..........",
	"....B...........",
	"...B............",
	"..B.............",
	".G..............",
	"G...............",
}

var heldItemFixturePalette = map[byte]color.NRGBA{
	'C': {R: 70, G: 200, B: 220, A: 255},
	'R': {R: 230, G: 60, B: 60, A: 255},
	'B': {R: 140, G: 90, B: 50, A: 255},
	'G': {R: 240, G: 220, B: 60, A: 255},
}

// heldItemFixtureSpec describes one server item; offsets is its render offsets component value.
type heldItemFixtureSpec struct {
	id           string
	name         string
	texels       int
	handEquipped bool
	offsets      any
}

// heldItemFixtureSpecs covers the default grip and each way a server declares render offsets.
var heldItemFixtureSpecs = []heldItemFixtureSpec{
	{id: "fixture:held_flat", name: "Held Flat", texels: 16},
	{id: "fixture:held_tool", name: "Held Tool", texels: 16, handEquipped: true},
	{id: "fixture:held_large", name: "Held Large", texels: 32, handEquipped: true},
	{id: "fixture:held_scaled", name: "Held Scaled", texels: 32, handEquipped: true, offsets: map[string]any{
		"main_hand": heldOffsetPerspectives(
			map[string]any{"scale": []float32{0.0375, 0.0625, 0.0375}},
			map[string]any{"scale": []float32{0.0375, 0.0625, 0.0375}},
		),
		"off_hand": heldOffsetPerspectives(
			map[string]any{"scale": []float32{0.0375, 0.0625, 0.0375}},
			map[string]any{"scale": []float32{0.0375, 0.0625, 0.0375}},
		),
	}},
	{id: "fixture:held_placed", name: "Held Placed", texels: 16, handEquipped: true, offsets: map[string]any{
		"main_hand": heldOffsetPerspectives(
			heldOffset([3]float32{0.3, -0.2, -0.8}, [3]float32{0, -30, 45}, [3]float32{0.08, 0.04, 0.04}),
			heldOffset([3]float32{0, 0.6, -0.4}, [3]float32{-90, 0, 180}, [3]float32{0.08, 0.04, 0.04}),
		),
		"off_hand": heldOffsetPerspectives(
			heldOffset([3]float32{-0.6, 0, -0.8}, [3]float32{0, 30, -45}, [3]float32{0.03, 0.06, 0.03}),
			heldOffset([3]float32{0, 0.3, 0}, [3]float32{90, 0, 0}, [3]float32{0.03, 0.06, 0.03}),
		),
	}},
	{id: "fixture:held_preset", name: "Held Preset", texels: 16, handEquipped: true, offsets: "tools"},
}

func heldOffsetPerspectives(first, third map[string]any) map[string]any {
	return map[string]any{"first_person": first, "third_person": third}
}

func heldOffset(position, rotation, scale [3]float32) map[string]any {
	return map[string]any{"position": position[:], "rotation": rotation[:], "scale": scale[:]}
}

// heldItemFixture is a server-defined item from heldItemFixtureSpecs. Items compare by value, so
// it holds the spec's index rather than the spec's component maps.
type heldItemFixture struct{ spec int }

var _ world.CustomItem = heldItemFixture{}

func (i heldItemFixture) EncodeItem() (string, int16) { return heldItemFixtureSpecs[i.spec].id, 0 }
func (i heldItemFixture) Name() string                { return heldItemFixtureSpecs[i.spec].name }
func (heldItemFixture) Category() category.Category   { return category.Equipment() }
func (heldItemFixture) OffHand() bool                 { return true }
func (i heldItemFixture) HandEquipped() bool          { return heldItemFixtureSpecs[i.spec].handEquipped }

// Texture scales the shared art up to the spec's icon size.
func (i heldItemFixture) Texture() image.Image {
	texels := heldItemFixtureSpecs[i.spec].texels
	factor := texels / len(heldItemFixtureArt)
	pixels := image.NewNRGBA(image.Rect(0, 0, texels, texels))
	for y := range texels {
		for x := range texels {
			pixels.SetNRGBA(x, y, heldItemFixturePalette[heldItemFixtureArt[y/factor][x/factor]])
		}
	}
	return pixels
}

// registerHeldItemFixture adds the items; it must run before the server builds its worlds.
func registerHeldItemFixture() {
	for spec := range heldItemFixtureSpecs {
		world.RegisterItem(heldItemFixture{spec: spec})
	}
}

// withHeldItemOffsets returns a registry with each spec's render offsets on its entry. Dragonfly
// has no custom item hook for this component, so the fixture edits the registry the way a server
// plugin would send it. Entries share their maps across connections, so edited ones are copied.
func withHeldItemOffsets(entries []protocol.ItemEntry) []protocol.ItemEntry {
	entries = slices.Clone(entries)
	offsets := make(map[string]any, len(heldItemFixtureSpecs))
	for _, spec := range heldItemFixtureSpecs {
		if spec.offsets != nil {
			offsets[spec.id] = spec.offsets
		}
	}
	for index, entry := range entries {
		value, declared := offsets[entry.Name]
		if !declared {
			continue
		}
		components, ok := entry.Data["components"].(map[string]any)
		if !ok {
			continue
		}
		components = maps.Clone(components)
		components[renderOffsetsComponent] = value
		data := maps.Clone(entry.Data)
		data["components"] = components
		entries[index].Data = data
	}
	return entries
}

// heldItemFixtureListener sends the fixture's render offsets to every joining client.
func heldItemFixtureListener(inner func(server.Config) (server.Listener, error)) func(server.Config) (server.Listener, error) {
	return func(conf server.Config) (server.Listener, error) {
		listener, err := inner(conf)
		if err != nil {
			return nil, err
		}
		return &heldItemListener{Listener: listener}, nil
	}
}

type heldItemListener struct{ server.Listener }

// Accept wraps the connection so its StartGame registry carries the render offsets.
func (l *heldItemListener) Accept() (session.Conn, error) {
	conn, err := l.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &heldItemConn{Conn: conn}, nil
}

// Disconnect unwraps only this fixture's connection before delegating to another listener.
func (l *heldItemListener) Disconnect(conn session.Conn, reason string) error {
	if wrapped, ok := conn.(*heldItemConn); ok {
		conn = wrapped.Conn
	}
	return l.Listener.Disconnect(conn, reason)
}

type heldItemConn struct{ session.Conn }

// StartGameContext sends the registry with the fixture's render offsets.
func (c *heldItemConn) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	data.Items = withHeldItemOffsets(data.Items)
	return c.Conn.StartGameContext(ctx, data)
}
