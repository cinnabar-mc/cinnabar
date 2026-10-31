//! Dried ghasts are a small body on six flat tentacles, turned rigidly with their direction.
//!
//! The body shows the top left of each face's art. The tentacles use the carried art, laid out
//! as box unwraps, and both are chosen by the rehydration level.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_grouped_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::cuboid_quads;
use assets::facts::{BlockProperty, PropertyDomain, StateValue, pack::PackBlock as B};
use {super::super::*, assets::BlockFace};

/// Model units per sixteenth of a block.
const PIXEL: i16 = 16;
/// UV units per texel of the 32-texel tentacle art.
const TENTACLE_TEXEL: u16 = 128;
/// How far the body's art stops short of its edges, keeping neighbouring texels out.
const BODY_INSET: u16 = 10;
const BODY_ART: u16 = 2560;
const BODY: ([i16; 3], [i16; 3]) = ([3, 0, 3], [13, 10, 13]);

/// A tentacle's bounds in sixteenths, the origin of its art, and whether it runs along z.
type Tentacle = ([i16; 3], [i16; 3], [u16; 2], bool);
const TENTACLES: [Tentacle; 6] = [
    ([5, 0, 0], [7, 1, 3], [0, 0], true),
    ([9, 0, 0], [11, 1, 3], [0, 4], true),
    ([13, 0, 5], [16, 1, 7], [10, 6], false),
    ([13, 0, 9], [16, 1, 11], [10, 9], false),
    ([0, 0, 9], [3, 1, 11], [10, 0], false),
    ([0, 0, 5], [3, 1, 7], [10, 3], false),
];

/// One face's art rectangle: `[left, right, top, bottom]` in UV units.
type Rect = [u16; 4];

/// Maps `rect` onto a side's corners: left to right as seen from outside, its top row on top.
fn side_uvs(face: BlockFace, positions: [[i16; 3]; 4], [u0, u1, v0, v1]: Rect) -> [[u16; 2]; 4] {
    let axis = if matches!(face, BlockFace::North | BlockFace::South) {
        0
    } else {
        2
    };
    let ascending = matches!(face, BlockFace::South | BlockFace::West);
    let top = positions.iter().map(|corner| corner[1]).max().unwrap_or(0);
    let low = positions
        .iter()
        .map(|corner| corner[axis])
        .min()
        .unwrap_or(0);
    positions.map(|corner| {
        [
            if (corner[axis] == low) == ascending {
                u0
            } else {
                u1
            },
            if corner[1] == top { v0 } else { v1 },
        ]
    })
}

/// Maps `rect` onto a top or bottom: the first corner at low x, and at low z on a top and high z
/// on a bottom.
fn flat_uvs(face: BlockFace, positions: [[i16; 3]; 4], rect: Rect, turned: bool) -> [[u16; 2]; 4] {
    let [u0, u1, v0, v1] = rect;
    let (u0, u1, v0, v1) = if turned {
        (u1, u0, v1, v0)
    } else {
        (u0, u1, v0, v1)
    };
    let low = |axis: usize| {
        positions
            .iter()
            .map(|corner| corner[axis])
            .min()
            .unwrap_or(0)
    };
    let (low_x, low_z) = (low(0), low(2));
    positions.map(|corner| {
        let first_z = (corner[2] == low_z) == (face == BlockFace::Up);
        [
            if corner[0] == low_x { u0 } else { u1 },
            if first_z { v0 } else { v1 },
        ]
    })
}

/// The art rectangle of one tentacle face.
fn tentacle_rect(face: BlockFace, [u, v]: [u16; 2], along_z: bool) -> Rect {
    let [width, depth] = if along_z { [2, 3] } else { [3, 2] };
    let (row, offsets) = if along_z {
        (
            v + 3,
            [
                (BlockFace::South, 0),
                (BlockFace::West, 2),
                (BlockFace::North, 5),
                (BlockFace::East, 7),
            ],
        )
    } else {
        (
            v + 2,
            [
                (BlockFace::West, 0),
                (BlockFace::South, 2),
                (BlockFace::East, 5),
                (BlockFace::North, 7),
            ],
        )
    };
    let scale = |texels: u16| texels * TENTACLE_TEXEL;
    match face {
        BlockFace::Up => [
            scale(u + depth),
            scale(u + depth + width),
            scale(v),
            scale(v + depth),
        ],
        BlockFace::Down => [
            scale(u + 5),
            scale(u + 5 + width),
            scale(v),
            scale(v + depth),
        ],
        _ => {
            let start = offsets
                .into_iter()
                .find_map(|(side, start)| (side == face).then_some(start))
                .unwrap_or(0);
            let span = if matches!(face, BlockFace::North | BlockFace::South) {
                width
            } else {
                depth
            };
            [
                scale(u + start),
                scale(u + start + span),
                scale(row),
                scale(row + 1),
            ]
        }
    }
}

fn cull_when_on_boundary(quad: &mut ModelQuad, face: BlockFace) {
    let (axis, edge) = match face {
        BlockFace::West => (0, 0),
        BlockFace::East => (0, 256),
        BlockFace::Down => (1, 0),
        BlockFace::Up => (1, 256),
        BlockFace::North => (2, 0),
        BlockFace::South => (2, 256),
    };
    if quad.positions.iter().all(|corner| corner[axis] == edge) {
        quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
    }
}

/// The face a quad's art faces after `steps` turns from south toward west, north and east.
fn turned_face(face: BlockFace, steps: usize) -> BlockFace {
    const RING: [BlockFace; 4] = [
        BlockFace::South,
        BlockFace::West,
        BlockFace::North,
        BlockFace::East,
    ];
    match RING.iter().position(|side| *side == face) {
        Some(index) => RING[(index + steps) % RING.len()],
        None => face,
    }
}

/// The quad's face identifier in [`MODEL_QUAD_FLAG_FACE_MASK`].
const fn face_flag(face: BlockFace) -> u32 {
    match face {
        BlockFace::Down => 1,
        BlockFace::Up => 2,
        BlockFace::West => 3,
        BlockFace::East => 4,
        BlockFace::North => 5,
        BlockFace::South => 6,
    }
}

/// Turns a quad a quarter `steps` times around the cell centre, keeping its art.
fn turn(quad: &mut ModelQuad, face: BlockFace, steps: usize) {
    for _ in 0..steps {
        for corner in &mut quad.positions {
            *corner = [256 - corner[2], corner[1], corner[0]];
        }
    }
    let turned = turned_face(face, steps);
    quad.flags = (quad.flags & !MODEL_QUAD_FLAG_FACE_MASK) | face_flag(turned);
    cull_when_on_boundary(quad, turned);
}

fn ghast_quads(materials: [u32; 6], tentacle: u32, steps: usize) -> Vec<ModelQuad> {
    let mut quads = Vec::with_capacity(7 * BlockFace::ALL.len());
    let scaled = |corner: [i16; 3]| corner.map(|axis| axis * PIXEL);
    let body = cuboid_quads(materials, scaled(BODY.0), scaled(BODY.1));
    for (face, mut quad) in BlockFace::ALL.into_iter().zip(body) {
        let rect = [
            BODY_INSET,
            BODY_ART - BODY_INSET,
            BODY_INSET,
            BODY_ART - BODY_INSET,
        ];
        quad.uvs = match face {
            BlockFace::Up | BlockFace::Down => flat_uvs(face, quad.positions, rect, false),
            _ => side_uvs(face, quad.positions, rect),
        };
        turn(&mut quad, face, steps);
        quads.push(quad);
    }
    for (min, max, art, along_z) in TENTACLES {
        let faces = cuboid_quads([tentacle; 6], scaled(min), scaled(max));
        for (face, mut quad) in BlockFace::ALL.into_iter().zip(faces) {
            let rect = tentacle_rect(face, art, along_z);
            quad.uvs = match face {
                BlockFace::Up | BlockFace::Down => flat_uvs(face, quad.positions, rect, true),
                _ => side_uvs(face, quad.positions, rect),
            };
            turn(&mut quad, face, steps);
            quads.push(quad);
        }
    }
    quads
}

/// A state's quarter turns from a south-facing ghast, in the direction's domain order.
fn turns(record: &RegistryRecord) -> Option<usize> {
    let StateValue::String(direction) =
        record.property(BlockProperty::MinecraftCardinalDirection)?
    else {
        return None;
    };
    let PropertyDomain::String(domain) = BlockProperty::MinecraftCardinalDirection.domain() else {
        return None;
    };
    domain.iter().position(|side| *side == direction.as_ref())
}

/// Whether the record is a dried ghast, which carries tentacle art beside its six body faces.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::DriedGhast.identifier()
}

/// Selects the tentacle art of a state's rehydration level.
pub(in crate::compiler) fn tentacle_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let variant = match record.property(BlockProperty::RehydrationLevel)? {
        StateValue::Int(level) => usize::try_from(*level).ok()?,
        _ => return None,
    };
    let key: Box<str> =
        pack_compile_input::pack::resolve_carried_face_key(&pack.blocks, record, BlockFace::Up)?
            .into();
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: Some((state_variant, state_variant)),
            flags: 0,
        },
        key,
    ))
}

/// Compiles one state from its body and tentacle art, leaving packs without them to the
/// provisional shape.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u32, usize), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let (Some(materials), Some(steps)) = (inputs.materials(record), turns(record)) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    // A pack without tentacle art draws them with its body art rather than losing the shape.
    let tentacle = tentacle_descriptor(inputs.pack, record)
        .and_then(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
        .unwrap_or(materials[BlockFace::Up as usize]);
    let key = (materials, tentacle, steps);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        // Native draws all 42 faces, which needs a second part beyond the per-template quad limit.
        let quads = ghast_quads(materials, tentacle, steps);
        let groups = quads
            .chunks(BlockFace::ALL.len())
            .map(<[ModelQuad]>::to_vec);
        let template =
            push_grouped_model_template(groups, storage)?.expect("a ghast always has a body");
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

    fn bounds(quads: &[ModelQuad]) -> ([i16; 3], [i16; 3]) {
        let mut low = [i16::MAX; 3];
        let mut high = [i16::MIN; 3];
        for corner in quads.iter().flat_map(|quad| quad.positions) {
            for axis in 0..3 {
                low[axis] = low[axis].min(corner[axis]);
                high[axis] = high[axis].max(corner[axis]);
            }
        }
        (low, high)
    }

    #[test]
    fn the_body_and_its_six_tentacles_stay_in_their_authored_boxes_when_unturned() {
        let quads = ghast_quads([1; 6], 2, 0);
        assert_eq!(quads.len(), 42);
        assert_eq!(bounds(&quads[..6]), ([48, 0, 48], [208, 160, 208]));
        assert_eq!(bounds(&quads[6..12]), ([80, 0, 0], [112, 16, 48]));
    }

    #[test]
    fn turning_moves_the_two_front_tentacles_to_the_named_side() {
        // A west-facing ghast has its two parallel tentacles against the east wall.
        let west = ghast_quads([1; 6], 2, 1);
        let (low, high) = bounds(&west[6..12]);
        assert_eq!((low[0], high[0]), (208, 256));
        // An east-facing ghast has them against the west wall.
        let east = ghast_quads([1; 6], 2, 3);
        let (low, high) = bounds(&east[6..12]);
        assert_eq!((low[0], high[0]), (0, 48));
    }

    #[test]
    fn only_faces_on_the_cell_boundary_can_be_culled() {
        for steps in 0..4 {
            for quad in ghast_quads([1; 6], 2, steps) {
                let culled = quad.flags & (MODEL_QUAD_FLAG_FACE_MASK << 4) != 0;
                let on_boundary = (0..3).any(|axis| {
                    quad.positions[0][axis] == quad.positions[2][axis]
                        && matches!(quad.positions[0][axis], 0 | 256)
                });
                assert!(!culled || on_boundary);
            }
        }
    }

    #[test]
    fn tentacle_art_follows_its_box_unwrap_and_turns_its_flat_faces_a_half_turn() {
        let up = tentacle_rect(BlockFace::Up, [0, 0], true);
        assert_eq!(up, [384, 640, 0, 384]);
        let side = tentacle_rect(BlockFace::West, [0, 4], true);
        assert_eq!(side, [256, 640, 896, 1024]);
        let positions = cuboid_quads([1; 6], [0, 0, 0], [32, 16, 48])[BlockFace::Up as usize];
        let uvs = flat_uvs(BlockFace::Up, positions.positions, up, true);
        assert_eq!(uvs[0], [640, 384], "low x and low z sample the far corner");
    }

    #[test]
    fn body_art_is_the_inset_top_left_of_each_face() {
        for quad in ghast_quads([1; 6], 2, 0).iter().take(6) {
            for [u, v] in quad.uvs {
                assert!(u == 10 || u == 2550);
                assert!(v == 10 || v == 2550);
            }
        }
    }
}
