//! Protocol 10's cell overlays: server WIT 0.10's overlay hosts and types as `loaded` carries
//! them, and the overlays of a cell as snapshot cells and `set_overlays` ops carry them.

use serde::{Deserialize, Serialize};

use super::{BlockState, Face, Permutation, StateDef, Visual};

/// A validated overlay type: a look that only overlays draw, with its states, visual and
/// permutations checked as a block's are. Asset paths are absolute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayDef {
    pub id: String,
    pub states: Vec<StateDef>,
    pub visual: Visual,
    pub permutations: Vec<Permutation>,
}

/// An Experience's overlays: the ids of its blocks that may carry them, and its overlay types.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayRegistration {
    pub hosts: Vec<String>,
    pub types: Vec<OverlayDef>,
}

/// One overlay of a cell: its slot, the face it turns to, and what it draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub slot: u8,
    pub face: Face,
    pub look: OverlayLook,
}

/// What an overlay draws: one of the Experience's overlay types in some of its states, a state it
/// does not name taking its first value, or an item flat on the face, `size` pixels wide and
/// centred `x` pixels right of and `y` pixels above the face's centre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OverlayLook {
    Visual {
        kind: String,
        states: Vec<BlockState>,
    },
    Item {
        id: String,
        metadata: u16,
        size: f32,
        x: f32,
        y: f32,
    },
}
