package experience

import (
	"encoding/json"
	"errors"
	"fmt"
)

// The Go mirror of protocol 10's cell overlays in crates/experience-runtime/src/protocol/
// overlays.rs: server WIT 0.10's overlay hosts and types, which Loaded carries, and the overlays
// of a cell, which snapshot cells and SetOverlaysOp carry. The goldens keep it exact.

// OverlayDef is a validated overlay type: a look that only overlays draw, its states, visual and
// permutations checked as a block's are. Asset paths are absolute.
type OverlayDef struct {
	ID           string        `json:"id"`
	States       []StateDef    `json:"states"`
	Visual       Visual        `json:"visual"`
	Permutations []Permutation `json:"permutations"`
}

// OverlayRegistration is an Experience's overlays: the ids of its blocks that may carry them, and
// its overlay types.
type OverlayRegistration struct {
	Hosts []string     `json:"hosts"`
	Types []OverlayDef `json:"types"`
}

// Overlay is one overlay of a cell: its slot, the face it turns to, and what it draws.
type Overlay struct {
	Slot uint8       `json:"slot"`
	Face Face        `json:"face"`
	Look OverlayLook `json:"look"`
}

// Overlays are a cell's overlays in slot order; nil encodes as an empty list.
type Overlays []Overlay

// OverlayLook is what an overlay draws. Exactly one field is set.
type OverlayLook struct {
	Visual *OverlayVisual
	Item   *OverlayItem
}

// OverlayVisual is one of the Experience's overlay types, by id, in some of its states; a state
// it does not name takes its first value.
type OverlayVisual struct {
	Kind   string      `json:"kind"`
	States BlockStates `json:"states"`
}

// OverlayItem is an item drawn flat on the overlay's face: Size pixels wide, centred X pixels
// right of and Y pixels above the face's centre.
type OverlayItem struct {
	ID       string  `json:"id"`
	Metadata uint16  `json:"metadata"`
	Size     float32 `json:"size"`
	X        float32 `json:"x"`
	Y        float32 `json:"y"`
}

// SetOverlaysOp sets the whole overlay list, in slot order, of the owned overlay host at Pos,
// which keeps its states, data and generation.
type SetOverlaysOp struct {
	Pos      BlockPos `json:"pos"`
	Overlays Overlays `json:"overlays"`
}

// The "type" of the overlay union variants on the wire, as protocol.rs names them.
const (
	typeSetOverlays   = "set_overlays"
	typeOverlayVisual = "visual"
	typeOverlayItem   = "item"
)

type (
	setOverlaysWire struct {
		variantTag
		*SetOverlaysOp
	}
	overlayVisualWire struct {
		variantTag
		*OverlayVisual
	}
	overlayItemWire struct {
		variantTag
		*OverlayItem
	}
)

func (o Overlays) MarshalJSON() ([]byte, error) {
	if o == nil {
		return []byte("[]"), nil
	}
	return json.Marshal([]Overlay(o))
}

func (l OverlayLook) MarshalJSON() ([]byte, error) {
	switch {
	case l.Visual != nil && l.Item == nil:
		return json.Marshal(overlayVisualWire{variantTag{typeOverlayVisual}, l.Visual})
	case l.Item != nil && l.Visual == nil:
		return json.Marshal(overlayItemWire{variantTag{typeOverlayItem}, l.Item})
	}
	return nil, errors.New("an overlay look must be a visual or an item")
}

func (l *OverlayLook) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*l = OverlayLook{}
	switch tag {
	case typeOverlayVisual:
		return decodeStrict(data, &overlayVisualWire{OverlayVisual: fresh(&l.Visual)})
	case typeOverlayItem:
		return decodeStrict(data, &overlayItemWire{OverlayItem: fresh(&l.Item)})
	}
	return fmt.Errorf("unknown overlay look type %q", tag)
}
