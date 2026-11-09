//! Server WIT 0.10's cell overlays: reading and staging an overlay host's overlays, and which
//! overlay an interaction's click picks.

use anyhow::Result;
use block_transform::overlay_face::FaceTurn;
use block_transform::rotation::rotate_turns;

use super::{CallbackRes, Reach, stage};
use crate::host::cinnabar::experience_server::types::{self as wit, WorldError};
use crate::load::{Catalog, holds};
use crate::protocol::{
    BlockPos, BlockState, Condition, Face, Op, Overlay, OverlayDef, OverlayLook, Pixel, StateValue,
    StateValues,
};
use block_transform::overlay_face::{
    MAX_CELL_OVERLAYS, MAX_OVERLAY_ITEM_OFFSET, MAX_OVERLAY_ITEM_SIZE, OVERLAY_PICK_MARGIN,
};

/// A cell's edge in pixels.
const CELL_PIXELS: f32 = 16.0;

impl CallbackRes {
    /// The overlays of the block at `pos`, in slot order, with staged changes applied.
    pub(crate) fn overlays(
        &mut self,
        pos: BlockPos,
    ) -> Result<Result<Vec<wit::Overlay>, WorldError>> {
        self.host_call()?;
        Ok(self.snapshot.read(pos).map(|slot| {
            slot.overlays
                .iter()
                .cloned()
                .map(wit::Overlay::from)
                .collect()
        }))
    }

    /// Stages the whole overlay list of this Experience's overlay host at `pos`, sorted into
    /// slot order; see the WIT for the refusals. Restaging a cell replaces its staged op.
    pub(crate) fn set_overlays(
        &mut self,
        pos: BlockPos,
        overlays: Vec<wit::Overlay>,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let slot = match self.snapshot.write(pos, Reach::Network) {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !slot.owned {
            return Ok(Err(WorldError::NotOwned));
        }
        if !self.own.overlay_hosts.contains(&slot.id) {
            return Ok(Err(WorldError::UnsupportedState));
        }
        let overlays = match checked(&self.own, overlays) {
            Ok(overlays) => overlays,
            Err(error) => return Ok(Err(error)),
        };
        let op = Op::SetOverlays {
            pos,
            overlays: overlays.clone(),
        };
        match overlay_op(&self.ops, pos) {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        slot.overlays = overlays;
        Ok(Ok(()))
    }
}

/// The index of the staged `SetOverlays` at `pos`.
pub(super) fn overlay_op(ops: &[Op], pos: BlockPos) -> Option<usize> {
    ops.iter()
        .position(|op| matches!(op, Op::SetOverlays { pos: at, .. } if *at == pos))
}

/// `overlays` checked against the Experience's overlay types and items, in slot order.
fn checked(own: &Catalog, overlays: Vec<wit::Overlay>) -> Result<Vec<Overlay>, WorldError> {
    if overlays.len() > MAX_CELL_OVERLAYS {
        return Err(WorldError::TooLarge);
    }
    let mut taken = [false; MAX_CELL_OVERLAYS];
    let mut checked = Vec::with_capacity(overlays.len());
    for overlay in overlays {
        let slot = usize::from(overlay.slot);
        if slot >= MAX_CELL_OVERLAYS || std::mem::replace(&mut taken[slot], true) {
            return Err(WorldError::UnsupportedState);
        }
        let look = match overlay.look {
            wit::OverlayLook::Visual(wit::OverlayVisual { kind, states }) => {
                let def = own.overlays.get(&kind).ok_or(WorldError::UnknownBlock)?;
                let states: Vec<BlockState> = states.into_iter().map(BlockState::from).collect();
                for (i, state) in states.iter().enumerate() {
                    let axis = def.states.iter().find(|axis| axis.name == state.name);
                    let repeated = states[..i].iter().any(|earlier| earlier.name == state.name);
                    if repeated || !axis.is_some_and(|axis| holds(axis, &state.value)) {
                        return Err(WorldError::UnsupportedState);
                    }
                }
                OverlayLook::Visual { kind, states }
            }
            wit::OverlayLook::Item(wit::OverlayItem {
                item: wit::ItemKey { id, metadata },
                size,
                x,
                y,
            }) => {
                let own_item = metadata == 0 && own.items.contains_key(&id);
                let server_item = own
                    .server
                    .get(&id)
                    .is_some_and(|variants| variants.contains_key(&metadata));
                if !own_item && !server_item {
                    return Err(WorldError::UnknownBlock);
                }
                let placed = size.is_finite()
                    && size > 0.0
                    && size <= MAX_OVERLAY_ITEM_SIZE
                    && [x, y].iter().all(|offset| {
                        offset.is_finite() && offset.abs() <= MAX_OVERLAY_ITEM_OFFSET
                    });
                if !placed {
                    return Err(WorldError::UnsupportedState);
                }
                OverlayLook::Item {
                    id,
                    metadata,
                    size,
                    x,
                    y,
                }
            }
        };
        checked.push(Overlay {
            slot: overlay.slot,
            face: overlay.face.into(),
            look,
        });
    }
    checked.sort_by_key(|overlay| overlay.slot);
    Ok(checked)
}

/// The slot of the first of `overlays`, in slot order, whose selection box, turned to its face
/// and grown by [`OVERLAY_PICK_MARGIN`] on each side, holds `hit`. Items are never picked.
pub(super) fn target(own: &Catalog, overlays: &[Overlay], hit: Pixel) -> Option<u8> {
    let hit = [hit.x, hit.y, hit.z];
    overlays.iter().find_map(|overlay| {
        let OverlayLook::Visual { kind, states } = &overlay.look else {
            return None;
        };
        let def = own.overlays.get(kind)?;
        let (min, max) = selection(def, states)?;
        let turn = FaceTurn::of(overlay.face as u8)?;
        let (min, max) = turn.turn_box(min, max, CELL_PIXELS);
        (0..3)
            .all(|axis| {
                (min[axis] - OVERLAY_PICK_MARGIN..=max[axis] + OVERLAY_PICK_MARGIN)
                    .contains(&hit[axis])
            })
            .then_some(overlay.slot)
    })
}

/// The selection box of `def` in `states`, before its face's turn: its visual's, or the last
/// holding permutation's that sets one, under the rotation the visual or the last holding
/// permutation that sets one gives, about the cell's centre. None when nothing sets a box.
fn selection(def: &OverlayDef, states: &[BlockState]) -> Option<([f32; 3], [f32; 3])> {
    let mut selection = def.visual.selection;
    let mut rotation = def.visual.rotation;
    for permutation in &def.permutations {
        if condition_holds(&permutation.when, def, states) {
            selection = permutation.selection.or(selection);
            rotation = permutation.rotation.or(rotation);
        }
    }
    let selection = selection?;
    let corners = [selection.min, selection.max].map(|Pixel { x, y, z }| [x, y, z]);
    let turns = rotation.map_or([0; 3], |turns| [turns.x, turns.y, turns.z]);
    let half = CELL_PIXELS / 2.0;
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for bits in 0..8_usize {
        let corner = std::array::from_fn(|axis| corners[(bits >> axis) & 1][axis] - half);
        let turned = rotate_turns(corner, turns);
        for axis in 0..3 {
            min[axis] = min[axis].min(turned[axis] + half);
            max[axis] = max[axis].max(turned[axis] + half);
        }
    }
    Some((min, max))
}

/// Whether `when` holds for an overlay of `def` in `states`, a state they do not name taking
/// its first value.
fn condition_holds(when: &Condition, def: &OverlayDef, states: &[BlockState]) -> bool {
    when.iter().any(|clause| {
        clause.iter().all(|test| {
            let value = states
                .iter()
                .find(|state| state.name == test.state.name)
                .map(|state| state.value.clone())
                .or_else(|| {
                    let axis = def
                        .states
                        .iter()
                        .find(|axis| axis.name == test.state.name)?;
                    Some(match &axis.values {
                        StateValues::Bool => StateValue::Bool(false),
                        StateValues::Choices(choices) => StateValue::Choice(choices[0].clone()),
                    })
                });
            (value.as_ref() == Some(&test.state.value)) == test.equal
        })
    })
}

impl From<wit::Face> for Face {
    fn from(face: wit::Face) -> Self {
        match face {
            wit::Face::Down => Self::Down,
            wit::Face::Up => Self::Up,
            wit::Face::North => Self::North,
            wit::Face::South => Self::South,
            wit::Face::West => Self::West,
            wit::Face::East => Self::East,
        }
    }
}

impl From<Overlay> for wit::Overlay {
    fn from(overlay: Overlay) -> Self {
        Self {
            slot: overlay.slot,
            face: overlay.face.into(),
            look: match overlay.look {
                OverlayLook::Visual { kind, states } => {
                    wit::OverlayLook::Visual(wit::OverlayVisual {
                        kind,
                        states: states.into_iter().map(Into::into).collect(),
                    })
                }
                OverlayLook::Item {
                    id,
                    metadata,
                    size,
                    x,
                    y,
                } => wit::OverlayLook::Item(wit::OverlayItem {
                    item: wit::ItemKey { id, metadata },
                    size,
                    x,
                    y,
                }),
            },
        }
    }
}

impl From<Pixel> for wit::Pixel {
    fn from(Pixel { x, y, z }: Pixel) -> Self {
        Self { x, y, z }
    }
}
