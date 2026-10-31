//! Dripleaves: a crossed stem offset away from the facing, under a tilting leaf.
//!
//! Big dripleaves draw a leaf whose far edge falls with its tilt, skirted on three sides; small
//! dripleaves stack three open-bottomed cubes of leaf above their stem.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockProperty, PropertyDomain, StateValue, pack::PackBlock as B};
use {super::super::*, assets::BlockFace};

/// Model units per sixteenth of a block, and UV units per sixteenth of a tile.
const PIXEL: f32 = 16.0;
const UV_PIXEL: u16 = 256;
const FULL_UV: u16 = 4096;
const STEM_RADIUS: f32 = 3.0;
const LEAF_HEIGHT: f32 = 15.0;
/// The depth of art a skirt shows below the leaf's edge.
const SKIRT_V: u16 = 1024;
/// How far a side skirt's planes sit inside the cell.
const SIDE_INSET: f32 = 0.25;

/// A direction, as a quarter-turn index in the cardinal domain's order: south, west, north, east.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct Facing(u8);

impl Facing {
    /// The stem's centre in sixteenths, behind the leaf's middle.
    const fn stem_center(self) -> [f32; 2] {
        match self.0 {
            0 => [8.0, 4.0],
            1 => [12.0, 8.0],
            2 => [8.0, 12.0],
            _ => [4.0, 8.0],
        }
    }
}

/// How far a leaf's far edge falls, and how its skirts hang from it.
struct Tilt {
    leaf_edge: f32,
    front_bottom: f32,
    front_inset: f32,
    side_shift: f32,
    side_drop: f32,
}

/// Tilts in the tilt property's domain order; `unstable` draws like `none`.
const TILTS: [Tilt; 4] = [
    Tilt {
        leaf_edge: 15.0,
        front_bottom: 11.0,
        front_inset: 0.0,
        side_shift: 0.0,
        side_drop: 4.0,
    },
    Tilt {
        leaf_edge: 15.0,
        front_bottom: 11.0,
        front_inset: 0.0,
        side_shift: 0.0,
        side_drop: 4.0,
    },
    Tilt {
        leaf_edge: 8.0,
        front_bottom: 4.0,
        front_inset: 2.2875,
        side_shift: 1.9485,
        side_drop: 3.4933,
    },
    Tilt {
        leaf_edge: 0.0,
        front_bottom: -3.4036,
        front_inset: 3.4036,
        side_shift: 3.4036,
        side_drop: 2.1013,
    },
];

/// The state a dripleaf's geometry reads.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum Shape {
    BigStem(Facing),
    BigHead(Facing, u8),
    SmallLower(Facing),
    SmallUpper(Facing),
}

fn to_model([x, y, z]: [f32; 3]) -> [i16; 3] {
    [x, y, z].map(|axis| (axis * PIXEL).round() as i16)
}

fn surface(corners: [[f32; 3]; 4], uvs: [[u16; 2]; 4], material: u32) -> ModelQuad {
    ModelQuad {
        positions: corners.map(to_model),
        uvs,
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    }
}

/// The two crossed stem planes, `height` sixteenths tall.
fn stem(facing: Facing, height: f32, material: u32) -> [ModelQuad; 2] {
    let [x, z] = facing.stem_center();
    let r = STEM_RADIUS;
    let uvs = [
        [5 * UV_PIXEL, FULL_UV],
        [12 * UV_PIXEL, FULL_UV],
        [12 * UV_PIXEL, UV_PIXEL],
        [5 * UV_PIXEL, UV_PIXEL],
    ];
    [
        [[x - r, z + r], [x + r, z - r]],
        [[x - r, z - r], [x + r, z + r]],
    ]
    .map(|[[ax, az], [bx, bz]]| {
        surface(
            [
                [ax, 0.0, az],
                [bx, 0.0, bz],
                [bx, height, bz],
                [ax, height, az],
            ],
            uvs,
            material,
        )
    })
}

/// A leaf falling from the stem side to its far edge at `edge`, covering the whole cell.
fn leaf(facing: Facing, edge: f32, material: u32) -> ModelQuad {
    let (h, l) = (LEAF_HEIGHT, edge);
    let (top_low, top_high) = (0, FULL_UV);
    let (corners, uvs) = match facing.0 {
        0 => (
            [
                [0.0, l, 16.0],
                [16.0, l, 16.0],
                [16.0, h, 0.0],
                [0.0, h, 0.0],
            ],
            [
                [top_low, top_low],
                [top_high, top_low],
                [top_high, top_high],
                [top_low, top_high],
            ],
        ),
        2 => (
            [
                [0.0, h, 16.0],
                [16.0, h, 16.0],
                [16.0, l, 0.0],
                [0.0, l, 0.0],
            ],
            [
                [top_low, top_high],
                [top_high, top_high],
                [top_high, top_low],
                [top_low, top_low],
            ],
        ),
        3 => (
            [
                [0.0, h, 0.0],
                [16.0, l, 0.0],
                [16.0, l, 16.0],
                [0.0, h, 16.0],
            ],
            [
                [top_high, top_high],
                [top_high, top_low],
                [top_low, top_low],
                [top_low, top_high],
            ],
        ),
        _ => (
            [
                [0.0, l, 0.0],
                [16.0, h, 0.0],
                [16.0, h, 16.0],
                [0.0, l, 16.0],
            ],
            [
                [top_high, top_low],
                [top_high, top_high],
                [top_low, top_high],
                [top_low, top_low],
            ],
        ),
    };
    surface(corners, uvs, material)
}

/// A point at `along` sixteenths from the stem side toward the facing edge, `lateral` across.
fn in_facing(facing: Facing, along: f32, y: f32, lateral: f32) -> [f32; 3] {
    match facing.0 {
        0 => [lateral, y, along],
        1 => [16.0 - along, y, lateral],
        2 => [lateral, y, 16.0 - along],
        _ => [along, y, lateral],
    }
}

/// The skirt below the far edge and the two below the leaf's sides.
fn skirts(facing: Facing, tilt: &Tilt, front: u32, side: u32) -> [ModelQuad; 3] {
    let edge = tilt.leaf_edge;
    let front_quad = surface(
        [
            in_facing(facing, 16.0, edge, 0.0),
            in_facing(facing, 16.0, edge, 16.0),
            in_facing(facing, 16.0 - tilt.front_inset, tilt.front_bottom, 16.0),
            in_facing(facing, 16.0 - tilt.front_inset, tilt.front_bottom, 0.0),
        ],
        [[FULL_UV, 0], [0, 0], [0, SKIRT_V], [FULL_UV, SKIRT_V]],
        front,
    );
    let side_quad = |lateral: f32| {
        surface(
            [
                in_facing(facing, 0.0, LEAF_HEIGHT, lateral),
                in_facing(facing, 16.0, edge, lateral),
                in_facing(
                    facing,
                    16.0 - tilt.side_shift,
                    edge - tilt.side_drop,
                    lateral,
                ),
                in_facing(
                    facing,
                    -tilt.side_shift,
                    LEAF_HEIGHT - tilt.side_drop,
                    lateral,
                ),
            ],
            [[FULL_UV, 0], [0, 0], [0, SKIRT_V], [FULL_UV, SKIRT_V]],
            side,
        )
    };
    [
        front_quad,
        side_quad(SIDE_INSET),
        side_quad(16.0 - SIDE_INSET),
    ]
}

/// The lower-corner triple of a small dripleaf's three leaf cubes, in sixteenths.
const fn small_cubes(facing: Facing) -> [[f32; 3]; 3] {
    match facing.0 {
        0 => [[8.0, -5.0, -4.0], [0.0, 0.0, 4.0], [8.0, 6.0, 4.0]],
        1 => [[12.0, -5.0, 8.0], [4.0, 0.0, 0.0], [4.0, 6.0, 8.0]],
        2 => [[0.0, -5.0, 12.0], [8.0, 0.0, 4.0], [0.0, 6.0, 4.0]],
        _ => [[-4.0, -5.0, 0.0], [4.0, 0.0, 8.0], [4.0, 6.0, 0.0]],
    }
}

/// One small-dripleaf leaf cube: an art-mapped top and four walls, without a bottom.
fn small_cube(facing: Facing, [x, y, z]: [f32; 3], top: u32, wall: u32) -> [ModelQuad; 5] {
    const HALF: u16 = 2048;
    let [center_x, center_z] = facing.stem_center();
    let (x1, y1, z1) = (x + 8.0, y + 8.0, z + 8.0);
    // Art runs from the far side to the half tile on the side nearest the stem.
    let u_at = |at: f32, other: f32| {
        if (at - center_x).abs() < (other - center_x).abs() {
            HALF
        } else {
            0
        }
    };
    let v_at = |at: f32, other: f32| {
        if (at - center_z).abs() < (other - center_z).abs() {
            HALF
        } else {
            0
        }
    };
    let top_quad = surface(
        [[x, y1, z], [x, y1, z1], [x1, y1, z1], [x1, y1, z]],
        [
            [u_at(x, x1), v_at(z, z1)],
            [u_at(x, x1), v_at(z1, z)],
            [u_at(x1, x), v_at(z1, z)],
            [u_at(x1, x), v_at(z, z1)],
        ],
        top,
    );
    let wall_uvs = |left: u16, right: u16| [[left, HALF], [right, HALF], [right, 0], [left, 0]];
    [
        top_quad,
        surface(
            [[x, y, z], [x1, y, z], [x1, y1, z], [x, y1, z]],
            wall_uvs(HALF, 0),
            wall,
        ),
        surface(
            [[x, y, z1], [x1, y, z1], [x1, y1, z1], [x, y1, z1]],
            wall_uvs(0, HALF),
            wall,
        ),
        surface(
            [[x, y, z], [x, y, z1], [x, y1, z1], [x, y1, z]],
            wall_uvs(0, HALF),
            wall,
        ),
        surface(
            [[x1, y, z], [x1, y, z1], [x1, y1, z1], [x1, y1, z]],
            wall_uvs(HALF, 0),
            wall,
        ),
    ]
}

fn dripleaf_quads(materials: [u32; 6], shape: Shape) -> Vec<ModelQuad> {
    let material = |face: BlockFace| materials[face as usize];
    match shape {
        Shape::BigStem(facing) => stem(facing, 16.0, material(BlockFace::North)).to_vec(),
        Shape::BigHead(facing, tilt) => {
            let tilt = &TILTS[usize::from(tilt)];
            let mut quads = stem(facing, LEAF_HEIGHT, material(BlockFace::North)).to_vec();
            quads.push(leaf(facing, tilt.leaf_edge, material(BlockFace::South)));
            quads.extend(skirts(
                facing,
                tilt,
                material(BlockFace::Up),
                material(BlockFace::Down),
            ));
            quads
        }
        Shape::SmallLower(facing) => stem(facing, 16.0, material(BlockFace::South)).to_vec(),
        Shape::SmallUpper(facing) => {
            let mut quads = stem(facing, LEAF_HEIGHT, material(BlockFace::North)).to_vec();
            for corner in small_cubes(facing) {
                quads.extend(small_cube(
                    facing,
                    corner,
                    material(BlockFace::Up),
                    material(BlockFace::Down),
                ));
            }
            quads
        }
    }
}

fn domain_index(record: &RegistryRecord, property: BlockProperty) -> Option<u8> {
    let StateValue::String(value) = record.property(property)? else {
        return None;
    };
    let PropertyDomain::String(domain) = property.domain() else {
        return None;
    };
    u8::try_from(domain.iter().position(|name| *name == value.as_ref())?).ok()
}

fn flag(record: &RegistryRecord, property: BlockProperty) -> Option<bool> {
    match record.property(property)? {
        StateValue::Bool(value) => Some(*value),
        StateValue::Int(value @ 0..=1) => Some(*value == 1),
        _ => None,
    }
}

fn shape(record: &RegistryRecord) -> Option<Shape> {
    let facing = Facing(domain_index(
        record,
        BlockProperty::MinecraftCardinalDirection,
    )?);
    if record.name.as_ref() == B::BigDripleaf.identifier() {
        if flag(record, BlockProperty::BigDripleafHead)? {
            let tilt = domain_index(record, BlockProperty::BigDripleafTilt)?;
            Some(Shape::BigHead(facing, tilt))
        } else {
            Some(Shape::BigStem(facing))
        }
    } else if flag(record, BlockProperty::UpperBlockBit)? {
        Some(Shape::SmallUpper(facing))
    } else {
        Some(Shape::SmallLower(facing))
    }
}

/// Whether the record is a dripleaf, whose exact rule replaces its provisional envelope.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    let name = record.name.as_ref();
    name == B::BigDripleaf.identifier() || name == B::SmallDripleaf.identifier()
}

/// Compiles one dripleaf state from its six art slots, leaving packs without them to the next
/// rule.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], Shape), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let (Some(materials), Some(shape)) = (inputs.materials(record), shape(record)) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = (materials, shape);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            dripleaf_quads(materials, shape),
            0,
            storage.templates,
            storage.quads,
        )?;
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

    const MATERIALS: [u32; 6] = [1, 2, 3, 4, 5, 6];
    const SOUTH: Facing = Facing(0);

    fn heights(quad: &ModelQuad) -> Vec<i16> {
        quad.positions.iter().map(|corner| corner[1]).collect()
    }

    #[test]
    fn a_big_head_stops_its_stem_under_the_leaf_and_a_stalk_fills_its_cell() {
        let head = dripleaf_quads(MATERIALS, Shape::BigHead(SOUTH, 0));
        assert_eq!(*heights(&head[0]).iter().max().unwrap(), 240);
        let stalk = dripleaf_quads(MATERIALS, Shape::BigStem(SOUTH));
        assert_eq!(stalk.len(), 2);
        assert_eq!(*heights(&stalk[0]).iter().max().unwrap(), 256);
        assert_eq!(stalk[0].material, MATERIALS[BlockFace::North as usize]);
    }

    #[test]
    fn a_leaf_falls_to_its_facing_edge_with_its_tilt() {
        for (tilt, edge) in [(0, 240), (1, 240), (2, 128), (3, 0)] {
            for facing in 0..4 {
                let leaf = &dripleaf_quads(MATERIALS, Shape::BigHead(Facing(facing), tilt))[2];
                let lowest = leaf.positions.iter().map(|corner| corner[1]).min().unwrap();
                assert_eq!(lowest, edge, "tilt {tilt} facing {facing}");
                let [x, z] = match facing {
                    0 => [None, Some(256)],
                    1 => [Some(0), None],
                    2 => [None, Some(0)],
                    _ => [Some(256), None],
                };
                if tilt >= 2 {
                    for corner in leaf.positions {
                        if corner[1] == edge {
                            assert!(x.is_none_or(|x| corner[0] == x), "{facing} {corner:?}");
                            assert!(z.is_none_or(|z| corner[2] == z), "{facing} {corner:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_flat_leafs_skirts_hang_four_sixteenths_below_it() {
        let quads = dripleaf_quads(MATERIALS, Shape::BigHead(SOUTH, 0));
        for skirt in &quads[3..] {
            assert_eq!(*heights(skirt).iter().max().unwrap(), 240);
            assert_eq!(*heights(skirt).iter().min().unwrap(), 176);
        }
        assert_eq!(quads[3].material, MATERIALS[BlockFace::Up as usize]);
        assert_eq!(quads[4].material, MATERIALS[BlockFace::Down as usize]);
    }

    #[test]
    fn a_small_upper_stacks_three_open_cubes_that_may_leave_the_cell() {
        let quads = dripleaf_quads(MATERIALS, Shape::SmallUpper(SOUTH));
        assert_eq!(quads.len(), 2 + 3 * 5);
        let tops: Vec<i16> = quads[2..]
            .iter()
            .step_by(5)
            .map(|quad| quad.positions[0][1])
            .collect();
        assert_eq!(tops, [48, 128, 224]);
        assert!(
            quads
                .iter()
                .flat_map(|quad| quad.positions)
                .any(|corner| corner[2] < 0 || corner[1] < 0)
        );
        let lower = dripleaf_quads(MATERIALS, Shape::SmallLower(SOUTH));
        assert_eq!(lower.len(), 2);
        assert_eq!(lower[0].material, MATERIALS[BlockFace::South as usize]);
    }
}
