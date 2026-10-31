//! Trip wire is a flat two-sided strip that runs toward the state's connections.
//!
//! Each connected side draws its half of the strip in two segments. A wire reaching only one
//! side of an axis also draws the inner segment past the centre, and an unconnected wire runs
//! north to south.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockProperty, StateValue, pack::PackBlock as B};
use {super::super::*, assets::BlockFace};

const HEIGHT: i16 = 56;
const STRIP_LOW: i16 = 124;
const STRIP_HIGH: i16 = 132;
const FULL_U: u16 = 4096;
/// One art row band: the wire shows the rows of its two-row band.
const BAND: u16 = 512;

const NORTH: u8 = 1 << 0;
const EAST: u8 = 1 << 1;
const SOUTH: u8 = 1 << 2;
const WEST: u8 = 1 << 3;
const ATTACHED: u8 = 1 << 4;

/// The state bits a wire's geometry reads.
fn shape(record: &RegistryRecord) -> u8 {
    let flag = |property: BlockProperty, bit: u8| match record.property(property) {
        Some(StateValue::Bool(true) | StateValue::Int(1)) => bit,
        _ => 0,
    };
    flag(BlockProperty::MinecraftConnectionNorth, NORTH)
        | flag(BlockProperty::MinecraftConnectionEast, EAST)
        | flag(BlockProperty::MinecraftConnectionSouth, SOUTH)
        | flag(BlockProperty::MinecraftConnectionWest, WEST)
        | flag(BlockProperty::AttachedBit, ATTACHED)
}

/// The segments along each axis as `(start, end)` in model units, from a side's connection, its
/// opposite's and whether the crossing axis is connected.
fn segments(near: bool, far: bool, crossed: bool) -> impl Iterator<Item = (i16, i16)> {
    [
        (near, (0, 64)),
        (near || (far && !crossed), (64, 128)),
        (far || (near && !crossed), (128, 192)),
        (far, (192, 256)),
    ]
    .into_iter()
    .filter_map(|(drawn, range)| drawn.then_some(range))
}

fn trip_wire_quads(material: u32, shape: u8) -> Vec<ModelQuad> {
    let has = |bit: u8| shape & bit != 0;
    let (mut north, mut south) = (has(NORTH), has(SOUTH));
    let (east, west) = (has(EAST), has(WEST));
    if !(north || south || east || west) {
        (north, south) = (true, true);
    }
    let (low, high) = if has(ATTACHED) {
        (BAND, 2 * BAND)
    } else {
        (0, BAND)
    };
    let along_z = segments(north, south, east || west).map(|(start, end)| ModelQuad {
        positions: [
            [STRIP_LOW, HEIGHT, end],
            [STRIP_HIGH, HEIGHT, end],
            [STRIP_HIGH, HEIGHT, start],
            [STRIP_LOW, HEIGHT, start],
        ],
        uvs: [[0, low], [0, high], [FULL_U, high], [FULL_U, low]],
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    });
    let along_x = segments(west, east, north || south).map(|(start, end)| ModelQuad {
        positions: [
            [start, HEIGHT, STRIP_HIGH],
            [end, HEIGHT, STRIP_HIGH],
            [end, HEIGHT, STRIP_LOW],
            [start, HEIGHT, STRIP_LOW],
        ],
        uvs: [[0, high], [FULL_U, high], [FULL_U, low], [0, low]],
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    });
    along_z.chain(along_x).collect()
}

/// Whether the record is trip wire, whose exact rule replaces its provisional envelope.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::TripWire.identifier()
}

/// Compiles one wire state from its art; packs without that art keep the next rule's visual.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = (materials[BlockFace::Up as usize], shape(record));
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = trip_wire_quads(key.0, key.1);
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

    fn z_spans(shape: u8) -> Vec<[i16; 2]> {
        trip_wire_quads(1, shape)
            .iter()
            .filter(|quad| quad.positions[0][0] == STRIP_LOW)
            .map(|quad| [quad.positions[2][2], quad.positions[0][2]])
            .collect()
    }

    fn x_spans(shape: u8) -> Vec<[i16; 2]> {
        trip_wire_quads(1, shape)
            .iter()
            .filter(|quad| quad.positions[0][2] == STRIP_HIGH)
            .map(|quad| [quad.positions[0][0], quad.positions[1][0]])
            .collect()
    }

    #[test]
    fn an_unconnected_wire_runs_north_to_south_across_its_whole_cell() {
        assert_eq!(z_spans(0), [[0, 64], [64, 128], [128, 192], [192, 256]]);
        assert!(x_spans(0).is_empty());
    }

    #[test]
    fn a_lone_side_also_draws_past_the_centre_unless_the_other_axis_connects() {
        assert_eq!(z_spans(NORTH), [[0, 64], [64, 128], [128, 192]]);
        assert_eq!(z_spans(NORTH | EAST), [[0, 64], [64, 128]]);
        assert_eq!(x_spans(NORTH | EAST), [[128, 192], [192, 256]]);
    }

    #[test]
    fn attaching_moves_the_wire_to_the_next_art_band_and_every_quad_is_two_sided() {
        for (shape, rows) in [(0, [0, 512]), (ATTACHED, [512, 1024])] {
            for quad in trip_wire_quads(1, shape) {
                assert_ne!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
                assert!(quad.positions.iter().all(|corner| corner[1] == HEIGHT));
                let v = quad.uvs.map(|uv| uv[1]);
                assert_eq!(*v.iter().min().unwrap(), rows[0]);
                assert_eq!(*v.iter().max().unwrap(), rows[1]);
            }
        }
    }
}
