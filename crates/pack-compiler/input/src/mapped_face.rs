//! Cube blocks whose class turns its authored faces with its state.
//!
//! Each placed face reads one authored `blocks.json` slot and turns that slot's UVs by a
//! native flip (1 and 2 are opposite quarter turns, 3 a half turn, 4 a horizontal mirror).
//! Observers, crafters and vaults use fixed per-state tables; dispensers, droppers and piston
//! bases derive theirs from their facing, and jigsaws from a rotated frame.
//! Every table is indexed in the native facing order ([`BlockFace::FACING_ORDER`]).

mod jigsaw;

use assets::{
    BlockFace, RegistryRecord,
    facts::{BlockProperty, BlockRenderKind, PropertyDomain, StateValue, block_render_fact},
};

/// The block classes that map their faces by state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    Observer,
    /// Dispensers and droppers.
    Dispenser,
    Crafter,
    /// Piston and sticky-piston bodies; the head is drawn separately.
    PistonBase,
    Jigsaw,
    /// Vaults, whose cutout shell also draws an inward-facing interior.
    Vault,
}

/// The family that owns `record`'s face mapping, if any.
pub fn family(record: &RegistryRecord) -> Option<Family> {
    Some(match block_render_fact(&record.name).kind {
        BlockRenderKind::Observer => Family::Observer,
        BlockRenderKind::Dispenser => Family::Dispenser,
        BlockRenderKind::Crafter => Family::Crafter,
        BlockRenderKind::PistonBase => Family::PistonBase,
        BlockRenderKind::Jigsaw => Family::Jigsaw,
        BlockRenderKind::Vault => Family::Vault,
        _ => return None,
    })
}

/// One state's authored slot (as a native facing index) and UV flip for each world face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaceMapping {
    slots: [u8; 6],
    flips: [u8; 6],
}

impl FaceMapping {
    /// The authored `blocks.json` slot that world `face` draws.
    pub fn source_face(self, face: BlockFace) -> BlockFace {
        BlockFace::FACING_ORDER[usize::from(self.slots[face.facing_index()])]
    }

    /// The native UV flip applied to world `face`.
    pub fn flip(self, face: BlockFace) -> u8 {
        self.flips[face.facing_index()]
    }
}

/// Observer slots for each `minecraft:facing_direction`: the front (authored north) faces the
/// direction and the back (authored south) its opposite.
const OBSERVER_SLOTS: [[u8; 6]; 6] = [
    [2, 3, 1, 0, 5, 4],
    [3, 2, 1, 0, 5, 4],
    [0, 1, 2, 3, 4, 5],
    [0, 1, 3, 2, 4, 5],
    [0, 1, 5, 4, 2, 3],
    [0, 1, 5, 4, 3, 2],
];
const OBSERVER_FLIPS: [[u8; 6]; 6] = [
    [0, 0, 0, 0, 2, 1],
    [0, 0, 3, 3, 1, 2],
    [0, 3, 0, 0, 0, 3],
    [3, 0, 0, 0, 3, 0],
    [1, 1, 3, 0, 0, 0],
    [2, 2, 0, 3, 0, 0],
];

/// Crafter slots for each `orientation`, in the property's domain order.
const CRAFTER_SLOTS: [[u8; 6]; 12] = [
    [2, 3, 4, 5, 0, 1],
    [2, 3, 1, 0, 4, 5],
    [2, 3, 0, 1, 5, 4],
    [2, 3, 5, 4, 1, 0],
    [3, 2, 4, 5, 1, 0],
    [3, 2, 0, 1, 4, 5],
    [3, 2, 1, 0, 5, 4],
    [3, 2, 5, 4, 0, 1],
    [0, 1, 5, 4, 2, 3],
    [0, 1, 4, 5, 3, 2],
    [0, 1, 2, 3, 4, 5],
    [0, 1, 3, 2, 5, 4],
];
const CRAFTER_FLIPS: [[u8; 6]; 12] = [
    [1, 1, 1, 1, 0, 0],
    [3, 0, 0, 0, 2, 2],
    [0, 3, 0, 0, 1, 1],
    [2, 2, 2, 2, 0, 0],
    [2, 2, 2, 2, 3, 3],
    [0, 3, 3, 3, 1, 1],
    [3, 0, 3, 3, 2, 2],
    [1, 1, 1, 1, 3, 3],
    [1, 1, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0],
    [0, 3, 0, 0, 0, 0],
    [3, 0, 0, 0, 0, 0],
];

/// Piston body flips for each `facing_direction`; its slots follow the head direction.
const PISTON_FLIPS: [[u8; 6]; 6] = [
    [0, 0, 3, 3, 3, 3],
    [0; 6],
    [3, 3, 0, 0, 1, 1],
    [0, 0, 0, 0, 2, 2],
    [1, 1, 1, 1, 0, 0],
    [2, 2, 2, 2, 0, 0],
];

/// Vault slots for each `minecraft:cardinal_direction`: the front (authored north) faces
/// the direction.
const VAULT_SLOTS: [[u8; 6]; 4] = [
    [0, 1, 3, 2, 5, 4],
    [0, 1, 5, 4, 2, 3],
    [0, 1, 2, 3, 4, 5],
    [0, 1, 4, 5, 3, 2],
];
const VAULT_FLIPS: [[u8; 6]; 4] = [
    [3, 3, 0, 0, 0, 0],
    [1, 2, 0, 0, 0, 0],
    [0; 6],
    [2, 1, 0, 0, 0, 0],
];

/// Native facing indices of the authored slots the formula-driven families read.
const DOWN: u8 = 0;
const UP: u8 = 1;
const NORTH: u8 = 2;
const SOUTH: u8 = 3;
const EAST: u8 = 5;

/// Resolves `record`'s face mapping; `None` for other blocks and malformed states.
pub fn mapping(record: &RegistryRecord) -> Option<FaceMapping> {
    let table = |slots: &[[u8; 6]], flips: &[[u8; 6]], index: usize| {
        Some(FaceMapping {
            slots: *slots.get(index)?,
            flips: *flips.get(index)?,
        })
    };
    match family(record)? {
        Family::Observer => table(&OBSERVER_SLOTS, &OBSERVER_FLIPS, facing(record)?),
        Family::Crafter => table(
            &CRAFTER_SLOTS,
            &CRAFTER_FLIPS,
            domain_index(record, BlockProperty::Orientation)?,
        ),
        Family::Vault => table(
            &VAULT_SLOTS,
            &VAULT_FLIPS,
            domain_index(record, BlockProperty::MinecraftCardinalDirection)?,
        ),
        Family::Dispenser => {
            let facing = facing(record)?;
            Some(FaceMapping {
                slots: std::array::from_fn(|face| dispenser_slot(face, facing)),
                flips: [0; 6],
            })
        }
        Family::PistonBase => {
            let facing = facing(record)?;
            Some(FaceMapping {
                slots: std::array::from_fn(|face| piston_slot(face, facing)),
                flips: PISTON_FLIPS[facing],
            })
        }
        Family::Jigsaw => {
            let rotation = match record.property(BlockProperty::Rotation)? {
                StateValue::Int(rotation @ 0..=3) => *rotation as usize,
                _ => return None,
            };
            let (slots, flips) = jigsaw::mapping(facing(record)?, rotation);
            Some(FaceMapping { slots, flips })
        }
    }
}

/// The terrain array entry the class draws: lit observer backs, triggered and crafting
/// crafters, the vertical dispenser variant (whose single-entry arrays draw their first),
/// and the vault's state (ominous vaults use the second four entries).
pub fn texture_variant(record: &RegistryRecord) -> Option<usize> {
    let flag = |key| match record.property(key) {
        Some(StateValue::Bool(set)) => Some(*set),
        Some(StateValue::Int(value @ 0..=1)) => Some(*value == 1),
        _ => None,
    };
    Some(match family(record)? {
        Family::Observer => usize::from(flag(BlockProperty::PoweredBit)?),
        Family::Crafter if flag(BlockProperty::Crafting)? => 2,
        Family::Crafter => usize::from(flag(BlockProperty::TriggeredBit)?),
        Family::Dispenser => usize::from(facing(record)? < 2),
        Family::PistonBase | Family::Jigsaw => 0,
        Family::Vault => {
            domain_index(record, BlockProperty::VaultState)?
                + 4 * usize::from(flag(BlockProperty::Ominous)?)
        }
    })
}

/// The native facing index of `facing_direction`, or of the observer's named
/// `minecraft:facing_direction`.
fn facing(record: &RegistryRecord) -> Option<usize> {
    let facing = match record.property(BlockProperty::MinecraftFacingDirection) {
        Some(StateValue::String(name)) => match name.as_ref() {
            "down" => BlockFace::Down,
            "up" => BlockFace::Up,
            "north" => BlockFace::North,
            "south" => BlockFace::South,
            "west" => BlockFace::West,
            "east" => BlockFace::East,
            _ => return None,
        }
        .facing_index(),
        Some(_) => return None,
        None => match record.property(BlockProperty::FacingDirection)? {
            StateValue::Int(facing) => usize::try_from(*facing).ok()?,
            _ => return None,
        },
    };
    (facing < BlockFace::FACING_ORDER.len()).then_some(facing)
}

/// A string state's index in its property's value domain, which follows the native enum.
fn domain_index(record: &RegistryRecord, property: BlockProperty) -> Option<usize> {
    let StateValue::String(value) = record.property(property)? else {
        return None;
    };
    let PropertyDomain::String(values) = property.domain() else {
        return None;
    };
    values
        .iter()
        .position(|candidate| *candidate == value.as_ref())
}

/// Dispenser fronts use the authored south slot when horizontal and the east slot when
/// vertical; vertical bodies draw the top slot on every other face.
fn dispenser_slot(face: usize, facing: usize) -> u8 {
    let vertical = facing < 2;
    match (face == facing, vertical) {
        (true, true) => EAST,
        (true, false) => SOUTH,
        (false, true) => UP,
        (false, false) if face < 2 => UP,
        (false, false) => NORTH,
    }
}

/// Piston bodies draw the top slot toward the head, the bottom slot opposite and the side
/// slot elsewhere. Horizontal heads point opposite their `facing_direction`.
fn piston_slot(face: usize, facing: usize) -> u8 {
    const HEADS: [usize; 6] = [0, 1, 3, 2, 5, 4];
    let head = HEADS[facing];
    if face == head {
        UP
    } else if face == head ^ 1 {
        DOWN
    } else {
        SOUTH
    }
}
