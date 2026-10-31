//! Redstone wire is a flat patch of cross or line art that climbs the walls its state names, tinted
//! by the state's power.
//!
//! Wire with one axis of arms draws a full-length strip of line art; anything else draws the cross,
//! cut back on each side without an arm unless it has no arms at all.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockProperty, PropertyDomain, StateValue, pack::PackBlock as B};
use {super::super::*, assets::BlockFace};

const FLAT_Y: i16 = 4;
const WALL_INSET: i16 = 4;
const WALL_TOP: i16 = 262;
const STRIP_LOW: i16 = 96;
const STRIP_HIGH: i16 = 160;
const CROSS_TRIM: i16 = 80;
const FULL: i16 = 256;
const TILE_UV: u16 = 4096;

pub(in crate::compiler) type TemplateKey = ([u32; 2], Arms);

/// What a wire does toward one horizontal side.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum Arm {
    None,
    Side,
    Up,
}

impl Arm {
    fn connected(self) -> bool {
        self != Self::None
    }
}

/// A wire's arms toward each side.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct Arms {
    north: Arm,
    east: Arm,
    south: Arm,
    west: Arm,
}

/// Reads an arm by its position in the property's domain, listed from no arm to a climbing one.
fn arm(record: &RegistryRecord, property: BlockProperty) -> Option<Arm> {
    let (StateValue::String(value), PropertyDomain::String(domain)) =
        (record.property(property)?, property.domain())
    else {
        return None;
    };
    match domain.iter().position(|option| *option == value.as_ref())? {
        0 => Some(Arm::None),
        1 => Some(Arm::Side),
        _ => Some(Arm::Up),
    }
}

fn arms(record: &RegistryRecord) -> Option<Arms> {
    Some(Arms {
        north: arm(record, BlockProperty::RedstoneNorth)?,
        east: arm(record, BlockProperty::RedstoneEast)?,
        south: arm(record, BlockProperty::RedstoneSouth)?,
        west: arm(record, BlockProperty::RedstoneWest)?,
    })
}

/// The state's power, which only the descriptor reads.
fn signal(record: &RegistryRecord) -> Option<u32> {
    match record.property(BlockProperty::RedstoneSignal)? {
        StateValue::Int(signal) => u32::try_from(signal).ok(),
        _ => None,
    }
}

fn scale(position: i16) -> u16 {
    u16::try_from(position).unwrap_or(0).min(FULL as u16) * (TILE_UV / FULL as u16)
}

/// A flat quad whose art runs with X and Z, so a smaller patch shows the matching part of the tile.
fn flat(material: u32, [x0, x1]: [i16; 2], [z0, z1]: [i16; 2]) -> ModelQuad {
    let corner = |x: i16, z: i16| [scale(x), scale(z)];
    ModelQuad {
        positions: [
            [x1, FLAT_Y, z1],
            [x1, FLAT_Y, z0],
            [x0, FLAT_Y, z0],
            [x0, FLAT_Y, z1],
        ],
        uvs: [
            corner(x1, z1),
            corner(x1, z0),
            corner(x0, z0),
            corner(x0, z1),
        ],
        material,
        flags: 0,
    }
}

/// The strip along Z, whose line art runs along the strip.
fn north_south_strip(material: u32) -> ModelQuad {
    let corner = |x: i16, z: i16| [scale(z), scale(x)];
    ModelQuad {
        positions: [
            [STRIP_HIGH, FLAT_Y, FULL],
            [STRIP_HIGH, FLAT_Y, 0],
            [STRIP_LOW, FLAT_Y, 0],
            [STRIP_LOW, FLAT_Y, FULL],
        ],
        uvs: [
            corner(STRIP_HIGH, FULL),
            corner(STRIP_HIGH, 0),
            corner(STRIP_LOW, 0),
            corner(STRIP_LOW, FULL),
        ],
        material,
        flags: 0,
    }
}

/// A strip up the wall on one side, facing back into the cell. Line art runs up the wall.
fn wall(material: u32, side: BlockFace) -> ModelQuad {
    let near = WALL_INSET;
    let far = FULL - WALL_INSET;
    let (low, high) = (STRIP_LOW, STRIP_HIGH);
    let (positions, rows): ([[i16; 3]; 4], [i16; 4]) = match side {
        BlockFace::West => (
            [
                [near, WALL_TOP, high],
                [near, 0, high],
                [near, 0, low],
                [near, WALL_TOP, low],
            ],
            [low, low, high, high],
        ),
        BlockFace::East => (
            [
                [far, 0, high],
                [far, WALL_TOP, high],
                [far, WALL_TOP, low],
                [far, 0, low],
            ],
            [high, high, low, low],
        ),
        BlockFace::North => (
            [
                [high, 0, near],
                [high, WALL_TOP, near],
                [low, WALL_TOP, near],
                [low, 0, near],
            ],
            [high, high, low, low],
        ),
        _ => (
            [
                [high, WALL_TOP, far],
                [high, 0, far],
                [low, 0, far],
                [low, WALL_TOP, far],
            ],
            [low, low, high, high],
        ),
    };
    let up_the_wall = |corner: usize| {
        if positions[corner][1] == 0 {
            0
        } else {
            TILE_UV
        }
    };
    ModelQuad {
        positions,
        uvs: [0, 1, 2, 3].map(|corner| [up_the_wall(corner), scale(rows[corner])]),
        material,
        flags: 0,
    }
}

fn wire_quads(cross: u32, line: u32, arms: Arms) -> Vec<ModelQuad> {
    let (north, east, south, west) = (
        arms.north.connected(),
        arms.east.connected(),
        arms.south.connected(),
        arms.west.connected(),
    );
    let along_x = east || west;
    let along_z = north || south;
    let trim = |connected: bool, low: bool| match (connected, low) {
        (true, true) => 0,
        (true, false) => FULL,
        (false, true) => CROSS_TRIM,
        (false, false) => FULL - CROSS_TRIM,
    };
    let patch = match (along_x, along_z) {
        (true, false) => flat(line, [0, FULL], [STRIP_LOW, STRIP_HIGH]),
        (false, true) => north_south_strip(line),
        (false, false) => flat(cross, [0, FULL], [0, FULL]),
        (true, true) => flat(
            cross,
            [trim(west, true), trim(east, false)],
            [trim(north, true), trim(south, false)],
        ),
    };
    let walls = [
        (arms.west, BlockFace::West),
        (arms.east, BlockFace::East),
        (arms.north, BlockFace::North),
        (arms.south, BlockFace::South),
    ];
    std::iter::once(patch)
        .chain(
            walls
                .into_iter()
                .filter(|(arm, _)| *arm == Arm::Up)
                .map(|(_, side)| wall(line, side)),
        )
        .collect()
}

/// Whether the record is redstone wire.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::RedstoneWire.identifier()
}

/// Resolves a face's art with the state's power, in the cut-out material the wire draws with.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, 0)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT
                | assets::MATERIAL_FLAG_DISABLE_AO
                | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING
                | assets::redstone_wire::material_flags(signal(record)?)?,
        },
        key,
    ))
}

/// Compiles one wire state; packs without its art keep the next rule's visual.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let (Some(materials), Some(arms)) = (inputs.materials(record), arms(record)) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let cross = materials[BlockFace::Up as usize];
    let line = materials[BlockFace::Down as usize];
    let key = ([cross, line], arms);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = wire_quads(cross, line, arms);
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CROSS: u32 = 1;
    const LINE: u32 = 2;

    fn wire(north: Arm, east: Arm, south: Arm, west: Arm) -> Vec<ModelQuad> {
        wire_quads(
            CROSS,
            LINE,
            Arms {
                north,
                east,
                south,
                west,
            },
        )
    }

    fn span(quad: &ModelQuad, axis: usize) -> [i16; 2] {
        let values = quad.positions.map(|position| position[axis]);
        [*values.iter().min().unwrap(), *values.iter().max().unwrap()]
    }

    #[test]
    fn one_axis_of_arms_draws_a_full_length_line() {
        use Arm::{None as N, Side as S};
        for quads in [wire(N, S, N, N), wire(N, N, N, S), wire(N, S, N, S)] {
            assert_eq!(quads.len(), 1);
            assert_eq!(quads[0].material, LINE);
            assert_eq!(span(&quads[0], 0), [0, FULL]);
            assert_eq!(span(&quads[0], 2), [STRIP_LOW, STRIP_HIGH]);
        }
        for quads in [wire(S, N, N, N), wire(N, N, S, N), wire(S, N, S, N)] {
            assert_eq!(quads[0].material, LINE);
            assert_eq!(span(&quads[0], 0), [STRIP_LOW, STRIP_HIGH]);
            assert_eq!(span(&quads[0], 2), [0, FULL]);
        }
    }

    #[test]
    fn the_cross_is_whole_alone_and_cut_back_on_sides_without_arms() {
        use Arm::{None as N, Side as S};
        let alone = wire(N, N, N, N);
        assert_eq!(alone[0].material, CROSS);
        assert_eq!(span(&alone[0], 0), [0, FULL]);
        assert_eq!(span(&alone[0], 2), [0, FULL]);
        let corner = wire(S, S, N, N);
        assert_eq!(corner[0].material, CROSS);
        assert_eq!(span(&corner[0], 0), [CROSS_TRIM, FULL]);
        assert_eq!(span(&corner[0], 2), [0, FULL - CROSS_TRIM]);
        // The art trims with the patch, so a cut-back cross never stretches its arms.
        let us = corner[0].uvs.map(|uv| uv[0]);
        assert_eq!(*us.iter().min().unwrap(), scale(CROSS_TRIM));
        let tee = wire(S, S, N, S);
        assert_eq!(span(&tee[0], 0), [0, FULL]);
        assert_eq!(span(&tee[0], 2), [0, FULL - CROSS_TRIM]);
    }

    #[test]
    fn up_arms_climb_their_own_wall_in_line_art() {
        use Arm::{None as N, Side as S, Up as U};
        let east = wire(N, U, N, S);
        assert_eq!(east.len(), 2);
        assert_eq!(east[1].material, LINE);
        assert_eq!(span(&east[1], 0), [FULL - WALL_INSET; 2]);
        assert_eq!(span(&east[1], 1), [0, WALL_TOP]);
        let north = wire(U, N, S, N);
        assert_eq!(span(&north[1], 2), [WALL_INSET; 2]);
        for quad in &wire(U, U, U, U)[1..] {
            for (uv, position) in quad.uvs.iter().zip(quad.positions) {
                assert_eq!(uv[0] == 0, position[1] == 0, "art runs up the wall");
            }
        }
        assert_eq!(wire(U, U, U, U).len(), 5);
    }
}
