use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use super::state::{exact_tagged_byte, exact_tagged_int, exact_tagged_string};
use {
    super::super::*,
    assets::{BlockFace, door::DoorPose},
};

/// The three art rows, in 1/4096 tile units, that a door's top and bottom caps read.
const CAP_ROWS: u16 = 768;

/// A class keeps distinct topology even when a pack shares its artwork with another door.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    name: Box<str>,
    materials: [u32; 2],
}

/// Admits the complete typed door state and its matching compiled selectors.
fn state(record: &RegistryRecord) -> Option<DoorPose> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 4 {
        return None;
    }
    let direction = if let Some(value) =
        state.get(assets::facts::BlockProperty::MinecraftCardinalDirection.name())
    {
        match exact_tagged_string(value)? {
            "south" => 0,
            "west" => 1,
            "north" => 2,
            "east" => 3,
            _ => return None,
        }
    } else {
        exact_tagged_int(
            state.get(assets::facts::BlockProperty::Direction.name())?,
            3,
        )?
    };
    let open = exact_tagged_byte(state.get(assets::facts::BlockProperty::OpenBit.name())?, 1)?;
    let hinge = exact_tagged_byte(
        state.get(assets::facts::BlockProperty::DoorHingeBit.name())?,
        1,
    )?;
    let upper = exact_tagged_byte(
        state.get(assets::facts::BlockProperty::UpperBlockBit.name())?,
        1,
    )?;
    for (field, value) in [
        (ModelStateField::Orientation, direction),
        (ModelStateField::Open, u32::from(open)),
        (ModelStateField::Hinge, u32::from(hinge)),
        (ModelStateField::Flags, u32::from(upper) << 7),
    ] {
        if record.model_state.get(field)? != value {
            return None;
        }
    }
    Some(DoorPose {
        direction,
        open: open != 0,
        hinge_right: hinge != 0,
        upper: upper != 0,
    })
}

/// Compiles all poses so world meshing can combine selectors across door halves.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_door(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(pose) = state(record)
        && let (Some(lower), Some(upper)) = (
            inputs.material(record, BlockFace::Down),
            inputs.material(record, BlockFace::South),
        )
    {
        let key = TemplateKey {
            name: record.name.clone(),
            materials: [lower, upper],
        };
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "door template",
                }
            })?;
            for offset in 0..assets::door::TEMPLATE_COUNT {
                let pose = DoorPose::from_offset(offset).expect("bounded door pose");
                push_model_template(
                    door_quads(pose, lower, upper).to_vec(),
                    assets::MODEL_TEMPLATE_FLAG_DOOR,
                    storage.templates,
                    storage.quads,
                )?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(
            &mut visual,
            [if pose.upper { upper } else { lower }; 6],
            base + pose.offset(),
        );
        visual.variant = pose.offset() << 2;
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Builds one paired pose's cuboid: each half's sides read its own art, mirrored as the native
/// door mirrors them, while the caps always stretch the top rows of the upper art and the bottom
/// rows of the lower art across the leaf.
pub(in crate::compiler) fn door_quads(pose: DoorPose, lower: u32, upper: u32) -> [ModelQuad; 6] {
    let material = if pose.upper { upper } else { lower };
    let (min, max) = door_bounds(
        pose.direction,
        u32::from(pose.open),
        u32::from(pose.hinge_right),
    );
    let mut quads = vanilla_cuboid_quads([material; 6], min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        match face {
            BlockFace::Up => {
                quad.material = upper;
                set_cap_uvs(quad, pose, face, [CAP_ROWS, 0], max);
            }
            BlockFace::Down => {
                quad.material = lower;
                set_cap_uvs(quad, pose, face, [4096, 4096 - CAP_ROWS], max);
            }
            _ if mirrors_side_art(pose, face) => {
                for uv in &mut quad.uvs {
                    uv[0] = 4096 - uv[0];
                }
            }
            _ => {}
        }
    }
    quads
}

/// Whether the native door mirrors this side face's art. The rule depends only on the leaf's
/// swung direction, its open state and its hinge.
fn mirrors_side_art(pose: DoorPose, face: BlockFace) -> bool {
    let swung = match (pose.open, pose.hinge_right) {
        (true, true) => pose.direction + 3,
        (true, false) => pose.direction + 1,
        (false, _) => pose.direction,
    } & 3;
    let listed = match swung {
        0 => matches!(face, BlockFace::South | BlockFace::East),
        1 | 2 => matches!(face, BlockFace::South | BlockFace::West),
        _ => matches!(face, BlockFace::North | BlockFace::West),
    };
    pose.open ^ pose.hinge_right ^ listed
}

/// Stretches a strip of art rows over a cap: along the leaf's width, across its thickness, turned
/// by the leaf's direction and mirrored by its hinge. `[v0, v1]` are the strip's start and end rows.
fn set_cap_uvs(
    quad: &mut ModelQuad,
    pose: DoorPose,
    face: BlockFace,
    [v0, v1]: [u16; 2],
    max: [i16; 3],
) {
    let turn = match (pose.open, pose.hinge_right) {
        (true, true) => pose.direction + 1,
        (true, false) => pose.direction + 3,
        (false, _) => pose.direction,
    } & 3;
    let [u0, u1] = if pose.hinge_right {
        [4096, 0]
    } else {
        [0, 4096]
    };
    let down = face == BlockFace::Down;
    for (uv, position) in quad.uvs.iter_mut().zip(quad.positions) {
        let (x, z) = (position[0] == max[0], position[2] == max[2]);
        let (at_u1, at_v1) = match (down, turn) {
            (false, 0) => (z, !x),
            (false, 1) => (!x, !z),
            (false, 2) => (!z, x),
            (false, _) => (x, z),
            (true, 0) => (z, x),
            (true, 1) => (!x, z),
            (true, 2) => (!z, !x),
            (true, _) => (x, !z),
        };
        *uv = [if at_u1 { u1 } else { u0 }, if at_v1 { v1 } else { v0 }];
    }
}

/// Builds the native door plane in fixed-point model coordinates.
pub(in crate::compiler) fn door_bounds(
    direction: u32,
    open: u32,
    hinge: u32,
) -> ([i16; 3], [i16; 3]) {
    const THICKNESS: i16 = (assets::door::THICKNESS * 256.0 + 0.5) as i16;
    const HIGH: i16 = 256 - THICKNESS;
    let plane = DoorPose {
        direction,
        open: open != 0,
        hinge_right: hinge != 0,
        upper: false,
    }
    .plane();
    match plane {
        0 => ([0, 0, 0], [256, 256, THICKNESS]),
        1 => ([HIGH, 0, 0], [256, 256, 256]),
        2 => ([0, 0, HIGH], [256, 256, 256]),
        _ => ([0, 0, 0], [THICKNESS, 256, 256]),
    }
}
