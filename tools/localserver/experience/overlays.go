package experience

import (
	"errors"
	"fmt"
	"maps"
	"math"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

// overlaysBlockEntityID is the block entity id of an overlay host's overlays, which Cinnabar
// draws. Vanilla clients know no such block entity and draw the host alone.
const overlaysBlockEntityID = "CinnabarOverlays"

// overlaySet is what an Experience's overlay hosts may carry: its overlay types by id, and its
// own items, which an item overlay may show besides the server's.
type overlaySet struct {
	types map[string]*blockType
	items map[string]bool
}

// overlayList is an overlay host's overlays in slot order, as the protocol carries them and as
// the NBT of its block entity holds them. It is never changed once made, so HostBlocks share it.
type overlayList struct {
	overlays Overlays
	nbt      []any
}

// HostBlock is an Experience block that may carry overlays: its block entity holds them, so
// Dragonfly saves them with the chunk and sends them to clients with it, and again on every
// change. Two HostBlocks of one type and state hash alike whatever their overlays.
type HostBlock struct {
	Block
	overlays *overlayList
}

var _ world.NBTer = HostBlock{}

// EncodeNBT returns the host's block entity: its id, unmovable, the host state its overlays
// belong to, and its overlays in slot order. A client shows the overlays only while that state is
// the cell's block, so they never outlive a host that a block without a block entity replaced.
// Each call returns a new map, which Dragonfly adds the position to; the list is shared.
func (h HostBlock) EncodeNBT() map[string]any {
	list := []any{}
	if h.overlays != nil {
		list = h.overlays.nbt
	}
	return map[string]any{"id": overlaysBlockEntityID, "isMovable": uint8(0), "host": networkState(h.Block), "overlays": list}
}

// networkState is b as block entity NBT names a block state: its name, its states and the block
// version. Clients know a salted state only by its salted network hash, so its salt is with it.
func networkState(b world.Block) map[string]any {
	name, properties := b.EncodeBlock()
	if salt, ok := world.DefaultBlockRegistry.NetworkHashSalt(name); ok {
		properties = maps.Clone(properties)
		properties[world.NetworkHashSaltProperty] = salt
	}
	return map[string]any{"name": name, "states": properties, "version": chunk.CurrentBlockVersion}
}

// DecodeNBT returns the host with the overlays its saved block entity holds. An overlay that no
// longer checks, because the Experience changed its overlay types or items, is dropped.
func (h HostBlock) DecodeNBT(data map[string]any) any {
	entries, _ := data["overlays"].([]any)
	overlays := make(Overlays, 0, len(entries))
	for _, entry := range entries {
		o, ok := overlayOfNBT(entry)
		if !ok || (len(overlays) > 0 && overlays[len(overlays)-1].Slot >= o.Slot) {
			continue
		}
		if _, err := h.t.overlays.check(Overlays{o}); err == nil {
			overlays = append(overlays, o)
		}
	}
	list, err := h.t.overlays.check(overlays)
	if err != nil {
		list = nil
	}
	return HostBlock{Block: h.Block, overlays: list}
}

// list returns the overlays in l, none for a nil list.
func (l *overlayList) list() Overlays {
	if l == nil {
		return nil
	}
	return l.overlays
}

// ownBlock returns the Experience block that b is, and its overlays when it is an overlay host.
func ownBlock(b world.Block) (Block, *overlayList, bool) {
	switch b := b.(type) {
	case Block:
		return b, nil, true
	case HostBlock:
		return b.Block, b.overlays, true
	}
	return Block{}, nil, false
}

// inWorld returns b as it is set in a world: a HostBlock carrying overlays for an overlay host,
// else b itself.
func (b Block) inWorld(overlays *overlayList) world.Block {
	if b.t.overlays != nil {
		return HostBlock{Block: b, overlays: overlays}
	}
	return b
}

// errBadOverlay is an overlay list that an overlay host may not carry.
var errBadOverlay = errors.New("bad overlay")

// check returns overlays as a host of set carries them, or why it may not: at most
// maxCellOverlays, in distinct slots below maxCellOverlays in slot order, each one of set's types
// in states it takes or an item of the server's or set's own, at most maxOverlayItemSize wide and
// centred at most maxOverlayItemOffset off the face's centre. A nil set is a block that carries
// none.
func (set *overlaySet) check(overlays Overlays) (*overlayList, error) {
	if set == nil {
		return nil, fmt.Errorf("%w: the block carries no overlays", errBadOverlay)
	}
	if len(overlays) > maxCellOverlays {
		return nil, fmt.Errorf("%w: %d overlays, at most %d", errBadOverlay, len(overlays), maxCellOverlays)
	}
	list := &overlayList{overlays: overlays, nbt: make([]any, 0, len(overlays))}
	for i, o := range overlays {
		if int(o.Slot) >= maxCellOverlays || i > 0 && overlays[i-1].Slot >= o.Slot {
			return nil, fmt.Errorf("%w: slot %d out of order or above %d", errBadOverlay, o.Slot, maxCellOverlays-1)
		}
		face := faceIndex(o.Face)
		if face < 0 {
			return nil, fmt.Errorf("%w: face %q", errBadOverlay, o.Face)
		}
		entry := map[string]any{"slot": o.Slot, "face": uint8(face)}
		switch look := o.Look; {
		case look.Visual != nil:
			t, ok := set.types[look.Visual.Kind]
			if !ok {
				return nil, fmt.Errorf("%w: %q is not an overlay type of the Experience", errBadOverlay, look.Visual.Kind)
			}
			b, err := Block{t: t}.withStates(look.Visual.States)
			if err != nil {
				return nil, fmt.Errorf("%w: %v", errBadOverlay, err)
			}
			entry["block"] = networkState(b)
		case look.Item != nil:
			it := look.Item
			_, serverItem := world.ItemByName(it.ID, int16(it.Metadata))
			if it.Metadata > math.MaxInt16 || !(it.Metadata == 0 && set.items[it.ID]) && !serverItem {
				return nil, fmt.Errorf("%w: %s:%d is neither the server's item nor the Experience's", errBadOverlay, it.ID, it.Metadata)
			}
			size, x, y := float64(it.Size), float64(it.X), float64(it.Y)
			if !(size > 0 && size <= maxOverlayItemSize) || !(math.Abs(x) <= maxOverlayItemOffset) || !(math.Abs(y) <= maxOverlayItemOffset) {
				return nil, fmt.Errorf("%w: item size %v at %v, %v", errBadOverlay, it.Size, it.X, it.Y)
			}
			entry["item"] = map[string]any{"Name": it.ID, "Damage": int16(it.Metadata)}
			entry["size"], entry["x"], entry["y"] = it.Size, it.X, it.Y
		default:
			return nil, fmt.Errorf("%w: slot %d draws nothing", errBadOverlay, o.Slot)
		}
		list.nbt = append(list.nbt, entry)
	}
	return list, nil
}

// overlayOfNBT reads one saved overlay entry back as the protocol carries it.
func overlayOfNBT(entry any) (Overlay, bool) {
	m, ok := entry.(map[string]any)
	if !ok {
		return Overlay{}, false
	}
	slot, okSlot := m["slot"].(uint8)
	face, okFace := m["face"].(uint8)
	if !okSlot || !okFace || int(face) >= len(faces) {
		return Overlay{}, false
	}
	o := Overlay{Slot: slot, Face: faces[face]}
	if b, ok := m["block"].(map[string]any); ok {
		name, _ := b["name"].(string)
		properties, _ := b["states"].(map[string]any)
		states := make(BlockStates, 0, len(properties))
		for state, value := range properties {
			if state == world.NetworkHashSaltProperty {
				continue
			}
			switch v := value.(type) {
			case uint8:
				on := v != 0
				states = append(states, BlockState{Name: state, Value: StateValue{Bool: &on}})
			case string:
				states = append(states, BlockState{Name: state, Value: StateValue{Choice: &v}})
			default:
				return Overlay{}, false
			}
		}
		o.Look.Visual = &OverlayVisual{Kind: name, States: states}
		return o, true
	}
	it, ok := m["item"].(map[string]any)
	if !ok {
		return Overlay{}, false
	}
	name, okName := it["Name"].(string)
	damage, okDamage := it["Damage"].(int16)
	size, okSize := m["size"].(float32)
	x, okX := m["x"].(float32)
	y, okY := m["y"].(float32)
	if !okName || !okDamage || damage < 0 || !okSize || !okX || !okY {
		return Overlay{}, false
	}
	o.Look.Item = &OverlayItem{ID: name, Metadata: uint16(damage), Size: size, X: x, Y: y}
	return o, true
}

// faceIndex is f's index in the protocol's face order, which Bedrock's face ids share, or -1.
func faceIndex(f Face) int {
	for i, face := range faces {
		if face == f {
			return i
		}
	}
	return -1
}
