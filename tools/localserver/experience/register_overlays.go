package experience

import (
	"fmt"
	"strings"
)

// registerOverlays checks l's overlays as the runtime's load/overlays.rs does: at most
// maxOverlayTypes types, each in l's namespace without a block's id or another type's, checked as
// a block; and hosts that are l's blocks, each once, which then carry them. It returns l's overlay
// types, not yet registered with Dragonfly; l's blocks and items are already in r.
func (r *Registry) registerOverlays(l Loaded, cache *assetCache) ([]*blockType, error) {
	if l.Overlays == nil {
		return nil, nil
	}
	if n := len(l.Overlays.Types); n > maxOverlayTypes {
		return nil, fmt.Errorf("experience %q declares %d overlay types; the limit is %d", l.ID, n, maxOverlayTypes)
	}
	set := &overlaySet{
		types: make(map[string]*blockType, len(l.Overlays.Types)),
		items: make(map[string]bool, len(l.Items)),
	}
	for _, def := range l.Items {
		set.items[def.ID] = true
	}
	types := make([]*blockType, 0, len(l.Overlays.Types))
	for _, def := range l.Overlays.Types {
		_, block := r.types[def.ID]
		_, declared := r.overlays[def.ID]
		switch {
		case !strings.HasPrefix(def.ID, l.ID+":"):
			return nil, fmt.Errorf("experience %q: overlay type %q is outside its namespace", l.ID, def.ID)
		case block:
			return nil, fmt.Errorf("experience %q: overlay type %q has the id of a block", l.ID, def.ID)
		case declared:
			return nil, fmt.Errorf("experience %q: overlay type %q is declared twice", l.ID, def.ID)
		}
		t, err := overlayTypeOf(l.ID, def, cache)
		if err != nil {
			return nil, fmt.Errorf("experience %q: overlay type %q: %w", l.ID, def.ID, err)
		}
		r.overlays[def.ID] = t
		set.types[def.ID] = t
		types = append(types, t)
	}
	for _, host := range l.Overlays.Hosts {
		t, ok := r.types[host]
		switch {
		case !ok || t.exp != l.ID:
			return nil, fmt.Errorf("experience %q: overlay host %q is not one of its blocks", l.ID, host)
		case t.overlays != nil:
			return nil, fmt.Errorf("experience %q: overlay host %q is listed twice", l.ID, host)
		}
		t.overlays = set
	}
	return types, nil
}

// overlayTypeOf checks def, an overlay type of the Experience exp, as a block without placement
// traits that mining never breaks. Without a selection box of its own it is never selected,
// unless a permutation that holds sets one.
func overlayTypeOf(exp string, def OverlayDef, cache *assetCache) (*blockType, error) {
	visual := def.Visual
	t, err := blockTypeOf(exp, BlockDef{
		ID:           def.ID,
		DisplayName:  def.ID,
		Mining:       Mining{Unbreakable: &Unbreakable{}},
		States:       def.States,
		Visual:       &visual,
		Permutations: def.Permutations,
	}, cache)
	if err != nil {
		return nil, err
	}
	t.overlay = true
	t.visual.base.SelectionDisabled = def.Visual.Selection == nil
	return t, nil
}
