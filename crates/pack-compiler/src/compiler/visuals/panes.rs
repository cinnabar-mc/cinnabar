//! Thin fences: glass panes draw two-faced walls, and iron and copper bars one plane per arm.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::block_render_fact;
use {super::super::*, assets::BlockFace};

const CENTRE: i16 = 128;
/// Half the width of a pane, rounded from the native 0.0635 to the model's 1/256 grid.
const HALF_WIDTH: i16 = 16;
const CENTRE_MIN_UV: u16 = 1792;
const CENTRE_MAX_UV: u16 = 2301;
const FULL_UV: u16 = 4096;
const SIDE_ROWS: [u16; 2] = [0, FULL_UV];

/// One connection direction in native order: north, east, south, west.
#[derive(Clone, Copy)]
struct Arm {
    /// Unit step away from the centre, as `[x, z]`.
    outward: [i16; 2],
    /// Unit step along the arm's left-hand wall, as `[x, z]`.
    across: [i16; 2],
    /// Connection mask bit.
    bit: u32,
    /// Column of the body art at the post and at the far end of the arm.
    columns: [u16; 2],
    /// Shade of the arm's walls and of the cap facing `outward`.
    wall_face: BlockFace,
    cap_face: BlockFace,
}

const ARMS: [Arm; 4] = [
    Arm {
        outward: [0, -1],
        across: [1, 0],
        bit: 1,
        columns: [CENTRE_MAX_UV, FULL_UV],
        wall_face: BlockFace::East,
        cap_face: BlockFace::North,
    },
    Arm {
        outward: [1, 0],
        across: [0, 1],
        bit: 2,
        columns: [CENTRE_MAX_UV, FULL_UV],
        wall_face: BlockFace::South,
        cap_face: BlockFace::East,
    },
    Arm {
        outward: [0, 1],
        across: [-1, 0],
        bit: 4,
        columns: [CENTRE_MIN_UV, 0],
        wall_face: BlockFace::West,
        cap_face: BlockFace::South,
    },
    Arm {
        outward: [-1, 0],
        across: [0, -1],
        bit: 8,
        columns: [CENTRE_MIN_UV, 0],
        wall_face: BlockFace::North,
        cap_face: BlockFace::West,
    },
];

/// Metal bars draw each wall as one plane, which is also why their icon is flat.
fn draws_single_plane(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).flat_icon
}

/// The native face id that selects a model quad's directional shade.
const fn shade_id(face: BlockFace) -> u32 {
    match face {
        BlockFace::Down => 1,
        BlockFace::Up => 2,
        BlockFace::West => 3,
        BlockFace::East => 4,
        BlockFace::North => 5,
        BlockFace::South => 6,
    }
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 2], bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_pane(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let (Some(body), Some(edge)) = (
        inputs.material(record, BlockFace::Down),
        inputs.material(record, BlockFace::East),
    ) {
        let single_plane = draws_single_plane(record);
        let key = ([body, edge], single_plane);
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "model template",
                }
            })?;
            for mask in 0..16 {
                push_model_template(
                    pane_quads(body, edge, mask, single_plane),
                    MODEL_TEMPLATE_FLAG_PANE,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(&mut visual, [body, body, edge, edge, body, body], base);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// A point `along` the arm and `across` it, as `[x, y, z]` in model units.
const fn at(arm: Arm, along: i16, across: i16, y: i16) -> [i16; 3] {
    [
        CENTRE + arm.outward[0] * along + arm.across[0] * across,
        y,
        CENTRE + arm.outward[1] * along + arm.across[1] * across,
    ]
}

/// Emits one connection mask, following the native arm, cap and centre rules.
pub(in crate::compiler) fn pane_quads(
    body: u32,
    edge: u32,
    mask: u32,
    single_plane: bool,
) -> Vec<ModelQuad> {
    debug_assert!(mask <= 15);
    let plane_flag = if single_plane {
        MODEL_QUAD_FLAG_TWO_SIDED
    } else {
        0
    };
    let wall = |positions: [[i16; 3]; 4], columns: [u16; 2], face: BlockFace| ModelQuad {
        positions,
        uvs: [
            [columns[0], SIDE_ROWS[0]],
            [columns[0], SIDE_ROWS[1]],
            [columns[1], SIDE_ROWS[1]],
            [columns[1], SIDE_ROWS[0]],
        ],
        material: body,
        flags: shade_id(face) | plane_flag,
    };
    let rail = |positions: [[i16; 3]; 4], uvs: [[u16; 2]; 4], face: BlockFace| ModelQuad {
        positions,
        uvs,
        material: edge,
        flags: shade_id(face),
    };
    let connections = mask.count_ones();
    let connected = |index: usize| mask & ARMS[index % 4].bit != 0;
    let mut centre_planes = [false; 4];
    let mut quads = Vec::new();
    for (index, arm) in ARMS.into_iter().enumerate() {
        if !connected(index) || connections == 4 {
            let cap = if !single_plane || (!connected(index + 3) && !connected(index + 1)) {
                Some(if connected(index) { CENTRE } else { HALF_WIDTH })
            } else if centre_planes[(index + 2) % 4] {
                None
            } else {
                centre_planes[index] = true;
                Some(0)
            };
            if let Some(along) = cap {
                quads.push(ModelQuad {
                    positions: [
                        at(arm, along, -HALF_WIDTH, 256),
                        at(arm, along, HALF_WIDTH, 256),
                        at(arm, along, HALF_WIDTH, 0),
                        at(arm, along, -HALF_WIDTH, 0),
                    ],
                    uvs: [
                        [CENTRE_MIN_UV, SIDE_ROWS[0]],
                        [CENTRE_MAX_UV, SIDE_ROWS[0]],
                        [CENTRE_MAX_UV, SIDE_ROWS[1]],
                        [CENTRE_MIN_UV, SIDE_ROWS[1]],
                    ],
                    material: body,
                    flags: shade_id(arm.cap_face) | plane_flag,
                });
            }
        }
        if !connected(index) {
            continue;
        }
        let side = |across: i16| {
            [
                at(arm, HALF_WIDTH, across, 256),
                at(arm, HALF_WIDTH, across, 0),
                at(arm, CENTRE, across, 0),
                at(arm, CENTRE, across, 256),
            ]
        };
        if single_plane {
            quads.push(wall(side(0), arm.columns, arm.wall_face));
        } else {
            let [near_top, near_bottom, far_bottom, far_top] = side(-HALF_WIDTH);
            quads.push(wall(side(HALF_WIDTH), arm.columns, arm.wall_face));
            quads.push(wall(
                [far_top, far_bottom, near_bottom, near_top],
                [arm.columns[1], arm.columns[0]],
                arm.wall_face,
            ));
        }
        quads.push(rail(
            [
                at(arm, CENTRE, -HALF_WIDTH, 256),
                at(arm, HALF_WIDTH, -HALF_WIDTH, 256),
                at(arm, HALF_WIDTH, HALF_WIDTH, 256),
                at(arm, CENTRE, HALF_WIDTH, 256),
            ],
            [
                [CENTRE_MIN_UV, FULL_UV],
                [CENTRE_MIN_UV, CENTRE_MAX_UV],
                [CENTRE_MAX_UV, CENTRE_MAX_UV],
                [CENTRE_MAX_UV, FULL_UV],
            ],
            BlockFace::Up,
        ));
        quads.push(rail(
            [
                at(arm, CENTRE, -HALF_WIDTH, 0),
                at(arm, CENTRE, HALF_WIDTH, 0),
                at(arm, HALF_WIDTH, HALF_WIDTH, 0),
                at(arm, HALF_WIDTH, -HALF_WIDTH, 0),
            ],
            [
                [CENTRE_MIN_UV, FULL_UV],
                [CENTRE_MAX_UV, FULL_UV],
                [CENTRE_MAX_UV, CENTRE_MAX_UV],
                [CENTRE_MIN_UV, CENTRE_MAX_UV],
            ],
            BlockFace::Down,
        ));
    }
    let [low, high] = [CENTRE - HALF_WIDTH, CENTRE + HALF_WIDTH];
    quads.push(rail(
        [
            [low, 256, high],
            [high, 256, high],
            [high, 256, low],
            [low, 256, low],
        ],
        [
            [CENTRE_MIN_UV, CENTRE_MAX_UV],
            [CENTRE_MAX_UV, CENTRE_MAX_UV],
            [CENTRE_MAX_UV, CENTRE_MIN_UV],
            [CENTRE_MIN_UV, CENTRE_MIN_UV],
        ],
        BlockFace::Up,
    ));
    quads.push(rail(
        [
            [low, 0, high],
            [low, 0, low],
            [high, 0, low],
            [high, 0, high],
        ],
        [
            [CENTRE_MIN_UV, CENTRE_MAX_UV],
            [CENTRE_MIN_UV, CENTRE_MIN_UV],
            [CENTRE_MAX_UV, CENTRE_MIN_UV],
            [CENTRE_MAX_UV, CENTRE_MAX_UV],
        ],
        BlockFace::Down,
    ));
    quads
}
