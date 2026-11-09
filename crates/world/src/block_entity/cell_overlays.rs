//! Cell overlays: the extra visuals an Experience's overlay host carries in its cell, which a
//! Cinnabar server sends as the host's `CinnabarOverlays` block entity:
//!
//! ```text
//! {id: "CinnabarOverlays", isMovable: 0b, host: {name, states, version},
//!  overlays: [{slot: byte, face: byte, block: {name, states, version}}
//!           | {slot: byte, face: byte, item: {Name, Damage: short}, size: float, x: float, y: float}]}
//! ```
//!
//! `host` is the block state the overlays belong to; they show only while that state is the cell's
//! block, so they never outlive a host that a block without a block entity replaced. `face` is a
//! Bedrock face id, `block` a block state whose model the overlay draws turned to that face, and
//! an `item` entry draws that item flat on the face, `size` pixels wide and centred `x`, `y`
//! pixels off the face's centre. Entries are in slot order, each slot below
//! [`MAX_CELL_OVERLAYS`](block_transform::overlay_face::MAX_CELL_OVERLAYS) once.

use std::any::Any;
use std::sync::{Arc, Mutex};

use block_transform::overlay_face::{
    MAX_CELL_OVERLAYS, MAX_OVERLAY_ITEM_OFFSET, MAX_OVERLAY_ITEM_SIZE,
};

use crate::{BlockEntityNbt, BlockIds, NbtCompound, NbtValue};

#[cfg(test)]
mod tests;

/// The block entity id of an overlay host's overlays.
pub const CELL_OVERLAYS_BLOCK_ENTITY_ID: &str = "CinnabarOverlays";

/// An item an overlay draws flat on its face: the item's network name and damage, its width in
/// pixels, and its centre's offset from the face's centre in pixels, right and up as the face is
/// seen from outside the cell.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayItem {
    pub name: Arc<str>,
    pub damage: i16,
    pub size: f32,
    pub x: f32,
    pub y: f32,
}

/// What one overlay draws once its block state is resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayLook {
    /// The model of the block state with this internal id.
    Block(u32),
    Item(OverlayItem),
}

/// One overlay of a cell: its slot, the Bedrock face it turns to, and what it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct CellOverlay {
    pub slot: u8,
    pub face: u8,
    pub look: OverlayLook,
}

/// A host's overlays with their block states resolved against one session's registry.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCellOverlays {
    /// The internal id of the host state the overlays belong to; air when it is unknown.
    pub host: u32,
    /// The overlays in slot order; an overlay whose block state is unknown is left out.
    pub overlays: Box<[CellOverlay]>,
}

impl ResolvedCellOverlays {
    /// The overlays, if `backing`, the internal id of the cell's block, is their host.
    #[must_use]
    pub fn on(&self, backing: u32) -> Option<&[CellOverlay]> {
        (self.host == backing).then_some(&*self.overlays)
    }
}

/// One saved entry before its block state is resolved.
#[derive(Debug)]
enum Look {
    Block(NbtCompound),
    Item(OverlayItem),
}

/// A `CinnabarOverlays` block entity, decoded once when its NBT is admitted. Resolving block
/// states needs the session's registry, so [`Self::resolve`] does it on first use and keeps the
/// result while that registry stays the same.
#[derive(Debug)]
pub struct CellOverlays {
    host: NbtCompound,
    entries: Box<[(u8, u8, Look)]>,
    resolved: Mutex<Option<ResolvedFor>>,
}

/// A resolution and the registry identity it was made against.
struct ResolvedFor {
    context: Arc<dyn Any + Send + Sync>,
    overlays: Arc<ResolvedCellOverlays>,
}

impl std::fmt::Debug for ResolvedFor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.overlays.fmt(formatter)
    }
}

impl CellOverlays {
    /// Decodes admitted NBT whose id is [`CELL_OVERLAYS_BLOCK_ENTITY_ID`]. An entry that breaks
    /// the format (a slot out of order or range, a face past east, an item too large or too far
    /// off centre) is skipped; a block entity without a host compound carries none.
    pub(super) fn from_nbt(nbt: &BlockEntityNbt) -> Option<Arc<Self>> {
        if nbt.id() != Some(CELL_OVERLAYS_BLOCK_ENTITY_ID) {
            return None;
        }
        let root = nbt.parse()?;
        let host = root.compound("host")?.clone();
        let mut entries = Vec::new();
        let mut next_slot = 0;
        for entry in root.list("overlays").unwrap_or_default() {
            let NbtValue::Compound(entry) = entry else {
                continue;
            };
            let Some(look) = entry_look(entry) else {
                continue;
            };
            let (Some(slot), Some(face)) = (entry.integer("slot"), entry.integer("face")) else {
                continue;
            };
            if !(next_slot..MAX_CELL_OVERLAYS as i64).contains(&slot) || !(0..6).contains(&face) {
                continue;
            }
            next_slot = slot + 1;
            entries.push((slot as u8, face as u8, look));
        }
        Some(Arc::new(Self {
            host,
            entries: entries.into_boxed_slice(),
            resolved: Mutex::default(),
        }))
    }

    /// Overlays the block entity holds, resolved or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the block entity holds no overlay.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The overlays with their host and block states resolved by `ids`, reused while `ids` keeps
    /// the registry identity of the last resolution.
    pub fn resolve(&self, ids: &dyn BlockIds) -> Arc<ResolvedCellOverlays> {
        let mut resolved = self
            .resolved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = resolved
            .as_ref()
            .filter(|previous| ids.inline_cache_context_matches(previous.context.as_ref()))
        {
            return Arc::clone(&previous.overlays);
        }
        let air = ids.air();
        let overlays = Arc::new(ResolvedCellOverlays {
            host: ids.resolve_persistent(&self.host),
            overlays: self
                .entries
                .iter()
                .filter_map(|(slot, face, look)| {
                    let look = match look {
                        Look::Block(state) => {
                            let id = ids.resolve_persistent(state);
                            (id != air).then_some(OverlayLook::Block(id))?
                        }
                        Look::Item(item) => OverlayLook::Item(item.clone()),
                    };
                    Some(CellOverlay {
                        slot: *slot,
                        face: *face,
                        look,
                    })
                })
                .collect(),
        });
        if let Some(context) = ids.inline_cache_context() {
            *resolved = Some(ResolvedFor {
                context,
                overlays: Arc::clone(&overlays),
            });
        }
        overlays
    }
}

/// The look of one saved entry: its block state, or its item placed within the face.
fn entry_look(entry: &NbtCompound) -> Option<Look> {
    if let Some(state) = entry.compound("block") {
        return Some(Look::Block(state.clone()));
    }
    let item = entry.compound("item")?;
    let name = item.string("Name").filter(|name| !name.is_empty())?;
    let damage = i16::try_from(item.integer("Damage").unwrap_or(0)).ok()?;
    let [size, x, y] = ["size", "x", "y"].map(|key| entry.float(key));
    let (size, x, y) = (size?, x?, y?);
    let placed = size > 0.0
        && size <= MAX_OVERLAY_ITEM_SIZE
        && [x, y]
            .iter()
            .all(|offset| offset.abs() <= MAX_OVERLAY_ITEM_OFFSET);
    placed.then(|| {
        Look::Item(OverlayItem {
            name: Arc::from(name),
            damage,
            size,
            x,
            y,
        })
    })
}
