//! The bell's supports. Terrain owns the frame and the block-entity renderer owns the swinging
//! body, so a bell shows its frame whether or not its block entity has arrived.
//!
//! Standing legs read the block's west texture and every wooden part its east texture. The
//! direction state runs a standing frame's bar across even directions, but runs a double-post
//! bar along them.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::{sprite_region, vanilla_cuboid_quads};
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// A rectangle of the block's art in texels, as `[u0, v0, u1, v1]`.
type Rect = [u16; 4];

/// The two crops a bar's long faces read; each direction assigns them to its own sides.
const BAR_NEAR: Rect = [2, 3, 14, 5];
const BAR_FAR: Rect = [2, 2, 14, 4];
const BAR_END: Rect = [5, 4, 7, 6];
const SIDE_BAR_END: Rect = [2, 0, 4, 2];
const STANDING_BAR_FAR_END: Rect = [0, 3, 4, 16];
const LEG_THIN: Rect = [0, 3, 2, 16];
const LEG_WIDE: Rect = [0, 3, 4, 16];
const LEG_CAP: Rect = [0, 0, 2, 4];

/// Top and bottom of the bar that holds a hanging body, in pixels.
const BAR_LOW: u16 = 13;
const BAR_HIGH: u16 = 15;
/// Top and bottom of a standing frame's bar and the top of its legs, in pixels.
const STANDING_BAR_LOW: u16 = 10;
const STANDING_BAR_HIGH: u16 = 12;
const LEG_HIGH: u16 = 13;

/// How a bell is mounted, in native `AttachmentType` order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Attachment {
    Standing,
    Hanging,
    Side,
    Multiple,
}

/// One frame cuboid in pixels and its per-face art.
struct Part {
    min: [u16; 3],
    max: [u16; 3],
    material: u32,
    /// Art per face, in [`BlockFace`] order.
    rects: [Rect; 6],
    /// Native flips of the down and up faces.
    flips: (u8, u8),
}

/// Identifies the bell by its render route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Bell
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads the mounting and direction a state draws its frame from.
fn pose(record: &RegistryRecord) -> Option<(Attachment, u8)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    let attachment = match super::state::exact_tagged_string(
        state.get(assets::facts::BlockProperty::Attachment.name())?,
    )? {
        "standing" => Attachment::Standing,
        "hanging" => Attachment::Hanging,
        "side" => Attachment::Side,
        "multiple" => Attachment::Multiple,
        _ => return None,
    };
    let direction = super::state::exact_tagged_int(
        state.get(assets::facts::BlockProperty::Direction.name())?,
        3,
    )? as u8;
    Some((attachment, direction))
}

/// Compiles every mounting pose from the pack's own face materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, u32, u8, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((attachment, direction)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let (Some(legs), Some(wood)) = (
        inputs.material(record, BlockFace::West),
        inputs.material(record, BlockFace::East),
    ) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    // The frame reads only these two; the other faces keep whatever art the pack resolves.
    let materials = BlockFace::ALL.map(|face| {
        inputs
            .material(record, face)
            .unwrap_or(assets::DIAGNOSTIC_MATERIAL)
    });
    let key = (legs, wood, attachment as u8, direction);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(legs, wood, attachment, direction),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Art per face from named sides, for a part whose two sides differ from its two ends.
fn rects(cap: Rect, west: Rect, east: Rect, north: Rect, south: Rect) -> [Rect; 6] {
    let mut rects = [cap; 6];
    rects[BlockFace::West as usize] = west;
    rects[BlockFace::East as usize] = east;
    rects[BlockFace::North as usize] = north;
    rects[BlockFace::South as usize] = south;
    rects
}

/// Builds the frame's quads for one pose.
fn quads(legs: u32, wood: u32, attachment: Attachment, direction: u8) -> Vec<ModelQuad> {
    parts(legs, wood, attachment, direction)
        .into_iter()
        .flat_map(|part| {
            let min = part.min.map(pixels);
            let max = part.max.map(pixels);
            let mut quads = vanilla_cuboid_quads([part.material; 6], min, max);
            for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
                let flip = match face {
                    BlockFace::Down => part.flips.0,
                    BlockFace::Up => part.flips.1,
                    _ => 0,
                };
                sprite_region(quad, part.rects[face as usize], flip, face);
            }
            quads
        })
        .collect()
}

/// Converts pixels to template position units.
const fn pixels(value: u16) -> i16 {
    (value * 16) as i16
}

/// The frame cuboids of one pose.
fn parts(legs: u32, wood: u32, attachment: Attachment, direction: u8) -> Vec<Part> {
    let odd = direction % 2 == 1;
    match attachment {
        Attachment::Standing => standing(legs, wood, direction),
        Attachment::Hanging => vec![Part {
            min: [7, 13, 7],
            max: [9, 16, 9],
            material: wood,
            rects: rects(
                [1, 3, 3, 5],
                [4, 2, 6, 5],
                [1, 2, 3, 5],
                [7, 2, 9, 5],
                [6, 2, 8, 5],
            ),
            flips: (0, 0),
        }],
        Attachment::Side => {
            let (low, high) = if matches!(direction, 0 | 3) {
                (0, 13)
            } else {
                (3, 16)
            };
            let (min, max) = bar_along(odd, low, high);
            let [west, east, north, south] = if odd {
                let (north, south) = if direction == 3 {
                    (BAR_NEAR, BAR_FAR)
                } else {
                    (BAR_FAR, BAR_NEAR)
                };
                [SIDE_BAR_END, SIDE_BAR_END, north, south]
            } else {
                let (west, east) = if direction == 0 {
                    (BAR_FAR, BAR_NEAR)
                } else {
                    (BAR_NEAR, BAR_FAR)
                };
                [west, east, SIDE_BAR_END, SIDE_BAR_END]
            };
            vec![Part {
                min,
                max,
                material: wood,
                rects: rects(BAR_NEAR, west, east, north, south),
                flips: match direction {
                    0 => (1, 2),
                    2 => (2, 1),
                    3 => (3, 3),
                    _ => (0, 0),
                },
            }]
        }
        Attachment::Multiple => {
            let (min, max) = bar_along(odd, 0, 16);
            let [west, east, north, south] = if odd {
                [BAR_END, BAR_END, BAR_NEAR, BAR_FAR]
            } else {
                [BAR_NEAR, BAR_FAR, BAR_END, BAR_END]
            };
            vec![Part {
                min,
                max,
                material: wood,
                rects: rects(BAR_NEAR, west, east, north, south),
                flips: match direction {
                    0 => (1, 2),
                    2 => (2, 1),
                    3 => (3, 3),
                    _ => (0, 0),
                },
            }]
        }
    }
}

/// Bounds of a hanging bar running along X when `along_x`, otherwise along Z.
fn bar_along(along_x: bool, low: u16, high: u16) -> ([u16; 3], [u16; 3]) {
    if along_x {
        ([low, BAR_LOW, 7], [high, BAR_HIGH, 9])
    } else {
        ([7, BAR_LOW, low], [9, BAR_HIGH, high])
    }
}

/// Two stone legs and the bar between them. The bar runs along X for even directions.
fn standing(legs: u32, wood: u32, direction: u8) -> Vec<Part> {
    let odd = direction % 2 == 1;
    let leg = |low: u16| {
        let high = low + 2;
        if odd {
            Part {
                min: [6, 0, low],
                max: [10, LEG_HIGH, high],
                material: legs,
                rects: rects(LEG_CAP, LEG_THIN, LEG_THIN, LEG_WIDE, LEG_WIDE),
                flips: (1, 1),
            }
        } else {
            Part {
                min: [low, 0, 6],
                max: [high, LEG_HIGH, 10],
                material: legs,
                rects: rects(LEG_CAP, LEG_WIDE, LEG_WIDE, LEG_THIN, LEG_THIN),
                flips: (0, 0),
            }
        }
    };
    let bar = if odd {
        Part {
            min: [7, STANDING_BAR_LOW, 2],
            max: [9, STANDING_BAR_HIGH, 14],
            material: wood,
            rects: rects(BAR_NEAR, BAR_NEAR, BAR_FAR, STANDING_BAR_FAR_END, BAR_FAR),
            flips: if direction == 1 { (2, 1) } else { (1, 2) },
        }
    } else {
        Part {
            min: [2, STANDING_BAR_LOW, 7],
            max: [14, STANDING_BAR_HIGH, 9],
            material: wood,
            rects: rects(BAR_NEAR, STANDING_BAR_FAR_END, BAR_FAR, BAR_FAR, BAR_NEAR),
            flips: if direction == 2 { (3, 3) } else { (0, 0) },
        }
    };
    vec![leg(14), bar, leg(0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::MODEL_QUAD_FLAG_FACE_MASK;

    const LEGS: u32 = 1;
    const WOOD: u32 = 2;

    /// The block-space pixel bounds of `quads`, one entry per six quads.
    fn cuboids(quads: &[ModelQuad]) -> Vec<([i16; 3], [i16; 3])> {
        quads
            .chunks(6)
            .map(|faces| {
                let corners = || faces.iter().flat_map(|quad| quad.positions);
                (
                    std::array::from_fn(|axis| corners().map(|p| p[axis]).min().unwrap() / 16),
                    std::array::from_fn(|axis| corners().map(|p| p[axis]).max().unwrap() / 16),
                )
            })
            .collect()
    }

    /// The UV extent of a quad on `face` in texels, whichever way it is turned.
    fn extent(quads: &[ModelQuad], face_id: u32) -> [u16; 4] {
        let quad = quads
            .iter()
            .find(|quad| quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face_id)
            .unwrap();
        let min = |axis: usize| quad.uvs.iter().map(|uv| uv[axis]).min().unwrap() / 256;
        let max = |axis: usize| quad.uvs.iter().map(|uv| uv[axis]).max().unwrap() / 256;
        [min(0), min(1), max(0), max(1)]
    }

    #[test]
    fn standing_frames_set_stone_legs_either_side_of_a_wooden_bar() {
        for (direction, expected) in [
            (
                0,
                [
                    ([14, 0, 6], [16, 13, 10]),
                    ([2, 10, 7], [14, 12, 9]),
                    ([0, 0, 6], [2, 13, 10]),
                ],
            ),
            (
                1,
                [
                    ([6, 0, 14], [10, 13, 16]),
                    ([7, 10, 2], [9, 12, 14]),
                    ([6, 0, 0], [10, 13, 2]),
                ],
            ),
        ] {
            for direction in [direction, direction + 2] {
                let quads = quads(LEGS, WOOD, Attachment::Standing, direction);
                assert_eq!(cuboids(&quads), expected);
                let materials: Vec<u32> = quads.chunks(6).map(|q| q[0].material).collect();
                assert_eq!(materials, [LEGS, WOOD, LEGS]);
            }
        }
    }

    #[test]
    fn hanging_bells_keep_one_wooden_rod_for_every_direction() {
        for direction in 0..4 {
            let quads = quads(LEGS, WOOD, Attachment::Hanging, direction);
            assert_eq!(cuboids(&quads), [([7, 13, 7], [9, 16, 9])]);
            assert!(quads.iter().all(|quad| quad.material == WOOD));
        }
    }

    #[test]
    fn side_bars_reach_the_wall_each_direction_stands_against() {
        for (direction, expected) in [
            (0, ([7, 13, 0], [9, 15, 13])),
            (1, ([3, 13, 7], [16, 15, 9])),
            (2, ([7, 13, 3], [9, 15, 16])),
            (3, ([0, 13, 7], [13, 15, 9])),
        ] {
            let quads = quads(LEGS, WOOD, Attachment::Side, direction);
            assert_eq!(cuboids(&quads), [expected]);
        }
    }

    #[test]
    fn double_bars_span_the_block_along_even_directions() {
        for (direction, expected) in [
            (0, ([7, 13, 0], [9, 15, 16])),
            (1, ([0, 13, 7], [16, 15, 9])),
            (2, ([7, 13, 0], [9, 15, 16])),
            (3, ([0, 13, 7], [16, 15, 9])),
        ] {
            let quads = quads(LEGS, WOOD, Attachment::Multiple, direction);
            assert_eq!(cuboids(&quads), [expected]);
        }
    }

    #[test]
    fn faces_read_the_cropped_rectangles_the_native_tessellator_assigns() {
        // Face ids: 1 down, 2 up, 3 west, 4 east, 5 north, 6 south.
        let rod = quads(LEGS, WOOD, Attachment::Hanging, 0);
        assert_eq!(extent(&rod, 5), [7, 2, 9, 5]);
        assert_eq!(extent(&rod, 6), [6, 2, 8, 5]);
        assert_eq!(extent(&rod, 3), [4, 2, 6, 5]);
        assert_eq!(extent(&rod, 4), [1, 2, 3, 5]);
        assert_eq!(extent(&rod, 2), [1, 3, 3, 5]);
        let side = quads(LEGS, WOOD, Attachment::Side, 1);
        assert_eq!(extent(&side, 6), BAR_NEAR);
        assert_eq!(extent(&side, 5), BAR_FAR);
        assert_eq!(extent(&side, 3), SIDE_BAR_END);
        let standing = quads(LEGS, WOOD, Attachment::Standing, 0);
        assert_eq!(extent(&standing, 5), LEG_THIN);
        assert_eq!(extent(&standing, 3), LEG_WIDE);
    }
}
