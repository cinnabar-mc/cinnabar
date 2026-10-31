use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

const FAMILY: u8 = 0;
const UPRIGHT: u8 = 0;
/// Height the top cap and an ordinary torch's sides reach above the torch's origin.
const CAP_HEIGHT: f32 = 0.61875;
/// Height a redstone torch's wider sides reach above its origin.
const REDSTONE_HEIGHT: f32 = 0.68125;
/// How far a wall torch's origin moves along its support axis, and how far up.
const WALL_OFFSET: f32 = 0.1;
const WALL_LIFT: f32 = 0.2;
/// How far a wall torch's foot leans towards its support relative to an upright foot.
const WALL_TILT: f32 = 0.4;
/// Top-cap shift per unit of tilt.
const CAP_TILT: f32 = 0.38125;
/// Side-top and foot shifts per unit of tilt; a redstone torch leans less above its foot.
const SIDE_TOP_TILT: f32 = CAP_HEIGHT * CAP_HEIGHT;
const REDSTONE_SIDE_TOP_TILT: f32 = SIDE_TOP_TILT * 0.908_257 * 0.945;
const FOOT_TILT: f32 = CAP_HEIGHT;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_torch(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(mount) = torch_mount(record)
    {
        let material = materials[BlockFace::Up as usize];
        let kind = TorchKind::of(record);
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: mount | (u8::from(kind == TorchKind::Redstone) << 7),
            materials: [material, 0],
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                torch_quads(material, kind, wall_placement(mount)),
                assets::MODEL_TEMPLATE_FLAG_TORCH,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Returns `UPRIGHT` or the `BlockFace` index of the supporting wall.
pub(in crate::compiler) fn torch_mount(record: &RegistryRecord) -> Option<u8> {
    let direction = canonical_state_str(
        &record.canonical_state,
        assets::facts::BlockProperty::TorchFacingDirection.name(),
    )?;
    Some(match direction.as_ref() {
        "unknown" | "top" => UPRIGHT,
        "west" => 1 + BlockFace::West as u8,
        "east" => 1 + BlockFace::East as u8,
        "north" => 1 + BlockFace::North as u8,
        "south" => 1 + BlockFace::South as u8,
        _ => return None,
    })
}

/// Which native torch outline a block draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::compiler) enum TorchKind {
    /// A 2x2 pixel post.
    Ordinary,
    /// Sides widened to four pixels and taller, showing the art's glowing head.
    Redstone,
}

impl TorchKind {
    fn of(record: &RegistryRecord) -> Self {
        let name = record.name.as_ref();
        if name == B::RedstoneTorch.identifier() || name == B::UnlitRedstoneTorch.identifier() {
            Self::Redstone
        } else {
            Self::Ordinary
        }
    }
}

/// Where a torch stands in its block, all in block fractions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::compiler) struct TorchPlacement {
    /// The foot's corner; the torch's axis stands half a block further along x and z.
    pub(in crate::compiler) origin: [f32; 3],
    /// Horizontal lean of the foot in x and z.
    pub(in crate::compiler) tilt: [f32; 2],
    /// Height the sides stop at. Above the origin it hides the foot and its art, as when a
    /// diode's plate covers the torch.
    pub(in crate::compiler) floor: f32,
}

/// The native placement of a torch in its own block.
fn wall_placement(mount: u8) -> TorchPlacement {
    let support = match mount {
        UPRIGHT => None,
        mount => Some(BlockFace::ALL[usize::from(mount - 1)]),
    };
    let (offset, tilt) = match support {
        None => {
            return TorchPlacement {
                origin: [0.0; 3],
                tilt: [0.0; 2],
                floor: 0.0,
            };
        }
        Some(BlockFace::West) => ([-WALL_OFFSET, 0.0], [-WALL_TILT, 0.0]),
        Some(BlockFace::East) => ([WALL_OFFSET, 0.0], [WALL_TILT, 0.0]),
        Some(BlockFace::North) => ([0.0, -WALL_OFFSET], [0.0, -WALL_TILT]),
        Some(_) => ([0.0, WALL_OFFSET], [0.0, WALL_TILT]),
    };
    TorchPlacement {
        origin: [offset[0], WALL_LIFT, offset[1]],
        tilt,
        floor: WALL_LIFT,
    }
}

fn units(block_fraction: f32) -> i16 {
    (block_fraction * 256.0).round() as i16
}

fn uv(fraction: f32) -> u16 {
    (fraction * 4096.0).round() as u16
}

/// Builds the native torch: four sheared side planes that read a column of the art, a top cap from
/// the art's cap pixels and, unless the floor hides it, a foot from the art's foot pixels.
pub(in crate::compiler) fn torch_quads(
    material: u32,
    kind: TorchKind,
    placement: TorchPlacement,
) -> Vec<ModelQuad> {
    let TorchPlacement {
        origin: [origin_x, origin_y, origin_z],
        tilt: [tilt_x, tilt_z],
        floor,
    } = placement;
    const HALF: f32 = 1.0 / 16.0;
    let (height, wing, top_v, u0, u1, top_tilt) = match kind {
        TorchKind::Ordinary => (CAP_HEIGHT, 0.0, 0.375, 0.4375, 0.5625, SIDE_TOP_TILT),
        TorchKind::Redstone => (
            REDSTONE_HEIGHT,
            HALF,
            0.3125,
            0.375,
            0.625,
            REDSTONE_SIDE_TOP_TILT,
        ),
    };
    let foot_tilt = top_tilt + FOOT_TILT;
    let (centre_x, centre_z) = (origin_x + 0.5, origin_z + 0.5);
    let (top, bottom) = (origin_y + height, floor);
    let (v0, v1) = (uv(top_v), uv(1.0 - (floor - origin_y)));
    let (u0, u1) = (uv(u0), uv(u1));
    // A side corner, as an offset from the sheared axis and whether it sits at the top.
    let side = |[x, z]: [f32; 2], at_top: bool| -> [i16; 3] {
        let (lean, y) = if at_top {
            (top_tilt, top)
        } else {
            (foot_tilt, bottom)
        };
        [
            units(centre_x + tilt_x * lean + x),
            units(y),
            units(centre_z + tilt_z * lean + z),
        ]
    };
    let across = HALF + wing;
    let side_quad = |face: u32, positions: [[i16; 3]; 4], uvs: [[u16; 2]; 4]| ModelQuad {
        positions,
        uvs,
        material,
        flags: face,
    };
    let (w, e) = (-HALF, HALF);
    let mut quads = vec![
        side_quad(
            3,
            [
                side([w, -across], false),
                side([w, across], false),
                side([w, across], true),
                side([w, -across], true),
            ],
            [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        ),
        side_quad(
            4,
            [
                side([e, -across], false),
                side([e, -across], true),
                side([e, across], true),
                side([e, across], false),
            ],
            [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
        ),
    ];
    if floor <= origin_y {
        let foot = |x: f32, z: f32| {
            [
                units(centre_x + tilt_x * foot_tilt + x),
                units(origin_y),
                units(centre_z + tilt_z * foot_tilt + z),
            ]
        };
        quads.push(cap_quad(
            1,
            [
                foot(-HALF, -HALF),
                foot(HALF, -HALF),
                foot(HALF, HALF),
                foot(-HALF, HALF),
            ],
            [[0, 0], [1, 0], [1, 1], [0, 1]],
            [0.8125, 0.9375],
            material,
        ));
    }
    let cap = |x: f32, z: f32| {
        [
            units(centre_x + tilt_x * CAP_TILT + x),
            units(origin_y + CAP_HEIGHT),
            units(centre_z + tilt_z * CAP_TILT + z),
        ]
    };
    quads.push(cap_quad(
        2,
        [
            cap(-HALF, -HALF),
            cap(-HALF, HALF),
            cap(HALF, HALF),
            cap(HALF, -HALF),
        ],
        [[0, 0], [0, 1], [1, 1], [1, 0]],
        [0.375, 0.5],
        material,
    ));
    quads.extend([
        side_quad(
            5,
            [
                side([-across, w], false),
                side([-across, w], true),
                side([across, w], true),
                side([across, w], false),
            ],
            [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
        ),
        side_quad(
            6,
            [
                side([-across, e], false),
                side([across, e], false),
                side([across, e], true),
                side([-across, e], true),
            ],
            [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        ),
    ]);
    quads
}

/// A cap reads the art's central two-pixel column; `corners` picks its ends and `rows` its rows.
fn cap_quad(
    face: u32,
    positions: [[i16; 3]; 4],
    corners: [[u8; 2]; 4],
    rows: [f32; 2],
    material: u32,
) -> ModelQuad {
    ModelQuad {
        positions,
        uvs: corners.map(|[u, v]| {
            [
                uv(if u == 0 { 0.4375 } else { 0.5625 }),
                uv(rows[usize::from(v)]),
            ]
        }),
        material,
        flags: face,
    }
}

#[cfg(test)]
mod tests {
    use {super::*, assets::BlockFace};

    fn quads(kind: TorchKind, mount: u8) -> Vec<ModelQuad> {
        torch_quads(7, kind, wall_placement(mount))
    }

    #[test]
    fn upright_torch_is_a_two_pixel_pillar_centred_in_the_block() {
        let quads = quads(TorchKind::Ordinary, UPRIGHT);
        assert_eq!(quads.len(), 6);
        let xs = quads.iter().flat_map(|q| q.positions.map(|p| p[0]));
        assert_eq!(xs.clone().min(), Some(112));
        assert_eq!(xs.max(), Some(144));
        assert!(
            quads
                .iter()
                .flat_map(|q| q.positions)
                .all(|p| (0..=158).contains(&p[1]))
        );
    }

    #[test]
    fn redstone_torch_sides_are_four_pixels_wide_and_rise_past_the_cap() {
        let quads = quads(TorchKind::Redstone, UPRIGHT);
        let side = quads.iter().find(|quad| quad.flags == 3).unwrap();
        let zs = side.positions.map(|p| p[2]);
        assert_eq!((zs.iter().min(), zs.iter().max()), (Some(&96), Some(&160)));
        assert_eq!(side.positions.map(|p| p[0]), [112; 4]);
        assert_eq!(side.positions.map(|p| p[1]).iter().max(), Some(&174));
        assert_eq!(side.uvs.map(|uv| uv[0]).iter().min(), Some(&1536));
        assert_eq!(side.uvs.map(|uv| uv[0]).iter().max(), Some(&2560));
        assert_eq!(side.uvs.map(|uv| uv[1]).iter().min(), Some(&1280));
        let cap = quads.iter().find(|quad| quad.flags == 2).unwrap();
        assert_eq!(cap.positions[0][1], 158);
    }

    #[test]
    fn wall_torch_foot_meets_its_support_and_lifts_off_the_floor() {
        for (support, axis, sign) in [
            (BlockFace::West, 0, -1.0_f32),
            (BlockFace::East, 0, 1.0),
            (BlockFace::North, 2, -1.0),
            (BlockFace::South, 2, 1.0),
        ] {
            let quads = quads(TorchKind::Ordinary, 1 + support as u8);
            let foot = quads.iter().find(|quad| quad.flags == 1).unwrap();
            let centre = foot
                .positions
                .map(|p| f32::from(p[axis]))
                .iter()
                .sum::<f32>()
                / 4.0;
            let expected =
                (0.5 + sign * (WALL_OFFSET + WALL_TILT * (SIDE_TOP_TILT + FOOT_TILT))) * 256.0;
            assert!((centre - expected).abs() <= 1.0, "{support:?}");
            assert_eq!(foot.positions[0][1], 51, "{support:?}");
            let top = quads.iter().find(|quad| quad.flags == 2).unwrap();
            let top_centre = top
                .positions
                .map(|p| f32::from(p[axis]))
                .iter()
                .sum::<f32>()
                / 4.0;
            assert_eq!(top_centre < centre, sign > 0.0, "{support:?}");
        }
    }

    #[test]
    fn clipped_torch_skips_its_foot_and_keeps_its_art_above_the_floor() {
        let quads = torch_quads(
            7,
            TorchKind::Redstone,
            TorchPlacement {
                origin: [0.0, -0.1875, 0.0],
                tilt: [0.0; 2],
                floor: 0.125,
            },
        );
        assert_eq!(quads.len(), 5);
        let side = quads.iter().find(|quad| quad.flags == 3).unwrap();
        assert_eq!(side.positions.map(|p| p[1]).iter().min(), Some(&32));
        assert_eq!(side.uvs.map(|uv| uv[1]).iter().max(), Some(&2816));
    }

    #[test]
    fn caps_read_the_sprites_cap_and_foot_pixels() {
        let quads = quads(TorchKind::Ordinary, UPRIGHT);
        let rows = |face: u32| {
            let quad = quads.iter().find(|quad| quad.flags == face).unwrap();
            let v = quad.uvs.map(|uv| uv[1]);
            (*v.iter().min().unwrap(), *v.iter().max().unwrap())
        };
        assert_eq!(rows(2), (1536, 2048));
        assert_eq!(rows(1), (3328, 3840));
    }
}
