//! Hollow hopper geometry with a state-selected downward or cardinal outlet.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// Identifies the terrain hopper before the provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Hopper
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads the typed facing selector while allowing unrelated hopper state.
fn facing(record: &RegistryRecord) -> Option<u8> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    super::state::exact_tagged_int(
        state.get(assets::facts::BlockProperty::FacingDirection.name())?,
        5,
    )
    .map(|value| value as u8)
}

/// Compiles the full hopper model and rotates its outlet with the block state.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(facing) = facing(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, facing);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            hopper_quads(materials, facing),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    // Keep fallback accounting until dimensional and replacement-pack acceptance is complete.
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

#[derive(Clone, Copy)]
enum TextureRole {
    Outside,
    Inside,
    Top,
}

#[derive(Clone, Copy)]
struct FaceSpec {
    face: BlockFace,
    texture: TextureRole,
    cullface: Option<BlockFace>,
}

/// Describes one cuboid face, its texture role and optional neighbour culling.
const fn face(face: BlockFace, texture: TextureRole, cullface: Option<BlockFace>) -> FaceSpec {
    FaceSpec {
        face,
        texture,
        cullface,
    }
}

/// Builds the seven-part hollow bowl and outlet. States 0 and 1 use the vertical
/// stem; states 2 through 5 use a four-pixel side outlet rotated toward the target face.
fn hopper_quads(materials: [u32; 6], facing: u8) -> Vec<ModelQuad> {
    let rotation = match facing {
        2 => 0,
        3 => 2,
        4 => 3,
        5 => 1,
        _ => 0,
    };
    let side_outlet = facing >= 2;
    let mut quads = Vec::with_capacity(32);

    append_box(
        &mut quads,
        materials,
        [0, 160, 0],
        [256, 176, 256],
        &[
            face(BlockFace::Down, TextureRole::Inside, None),
            face(BlockFace::Up, TextureRole::Inside, Some(BlockFace::Up)),
            face(BlockFace::West, TextureRole::Outside, Some(BlockFace::West)),
            face(BlockFace::East, TextureRole::Outside, Some(BlockFace::East)),
            face(
                BlockFace::North,
                TextureRole::Outside,
                Some(BlockFace::North),
            ),
            face(
                BlockFace::South,
                TextureRole::Outside,
                Some(BlockFace::South),
            ),
        ],
        rotation,
    );
    append_box(
        &mut quads,
        materials,
        [0, 176, 0],
        [32, 256, 256],
        &[
            face(BlockFace::Up, TextureRole::Top, Some(BlockFace::Up)),
            face(
                BlockFace::North,
                TextureRole::Outside,
                Some(BlockFace::North),
            ),
            face(
                BlockFace::South,
                TextureRole::Outside,
                Some(BlockFace::South),
            ),
            face(BlockFace::West, TextureRole::Outside, Some(BlockFace::West)),
            face(BlockFace::East, TextureRole::Outside, Some(BlockFace::Up)),
        ],
        rotation,
    );
    append_box(
        &mut quads,
        materials,
        [224, 176, 0],
        [256, 256, 256],
        &[
            face(BlockFace::Up, TextureRole::Top, Some(BlockFace::Up)),
            face(
                BlockFace::North,
                TextureRole::Outside,
                Some(BlockFace::North),
            ),
            face(
                BlockFace::South,
                TextureRole::Outside,
                Some(BlockFace::South),
            ),
            face(BlockFace::West, TextureRole::Outside, Some(BlockFace::Up)),
            face(BlockFace::East, TextureRole::Outside, Some(BlockFace::East)),
        ],
        rotation,
    );
    append_box(
        &mut quads,
        materials,
        [32, 176, 0],
        [224, 256, 32],
        &[
            face(BlockFace::Up, TextureRole::Top, Some(BlockFace::Up)),
            face(
                BlockFace::North,
                TextureRole::Outside,
                Some(BlockFace::North),
            ),
            face(BlockFace::South, TextureRole::Outside, Some(BlockFace::Up)),
        ],
        rotation,
    );
    append_box(
        &mut quads,
        materials,
        [32, 176, 224],
        [224, 256, 256],
        &[
            face(BlockFace::Up, TextureRole::Top, Some(BlockFace::Up)),
            face(BlockFace::North, TextureRole::Outside, Some(BlockFace::Up)),
            face(
                BlockFace::South,
                TextureRole::Outside,
                Some(BlockFace::South),
            ),
        ],
        rotation,
    );
    append_box(
        &mut quads,
        materials,
        [64, 64, 64],
        [192, 160, 192],
        &[
            face(BlockFace::Down, TextureRole::Inside, None),
            face(BlockFace::West, TextureRole::Outside, None),
            face(BlockFace::East, TextureRole::Outside, None),
            face(BlockFace::North, TextureRole::Outside, None),
            face(BlockFace::South, TextureRole::Outside, None),
        ],
        rotation,
    );
    if !side_outlet {
        append_box(
            &mut quads,
            materials,
            [96, 0, 96],
            [160, 64, 160],
            &[
                face(BlockFace::Down, TextureRole::Inside, Some(BlockFace::Down)),
                face(BlockFace::West, TextureRole::Outside, None),
                face(BlockFace::East, TextureRole::Outside, None),
                face(BlockFace::North, TextureRole::Outside, None),
                face(BlockFace::South, TextureRole::Outside, None),
            ],
            rotation,
        );
    }
    if side_outlet {
        append_box(
            &mut quads,
            materials,
            [96, 64, 0],
            [160, 128, 64],
            &[
                face(BlockFace::Down, TextureRole::Inside, None),
                face(BlockFace::Up, TextureRole::Outside, None),
                face(
                    BlockFace::North,
                    TextureRole::Outside,
                    Some(BlockFace::North),
                ),
                face(BlockFace::West, TextureRole::Outside, None),
                face(BlockFace::East, TextureRole::Outside, None),
            ],
            rotation,
        );
    }
    quads
}

/// Appends selected cuboid faces with rotated positions, materials and culling.
fn append_box(
    output: &mut Vec<ModelQuad>,
    materials: [u32; 6],
    min: [i16; 3],
    max: [i16; 3],
    faces: &[FaceSpec],
    rotation: u8,
) {
    let source = super::geometry::vanilla_cuboid_quads([0; 6], min, max);
    for spec in faces {
        let mut quad = source[spec.face as usize];
        quad.positions = quad.positions.map(|position| rotate(position, rotation));
        let target_face = rotate_face(spec.face, rotation);
        quad.material = match spec.texture {
            TextureRole::Outside => materials[target_face as usize],
            TextureRole::Inside => materials[BlockFace::Down as usize],
            TextureRole::Top => materials[BlockFace::Up as usize],
        };
        quad.flags = target_face.model_quad_face_id()
            | spec
                .cullface
                .map(|face| rotate_face(face, rotation).model_quad_face_id() << 4)
                .unwrap_or(0);
        output.push(quad);
    }
}

/// Turns a model-grid position clockwise around the block centre.
fn rotate([x, y, z]: [i16; 3], turns: u8) -> [i16; 3] {
    match turns % 4 {
        0 => [x, y, z],
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        _ => [z, y, 256 - x],
    }
}

/// Applies the same horizontal quarter turns to a material or culling face.
fn rotate_face(face: BlockFace, turns: u8) -> BlockFace {
    if matches!(face, BlockFace::Down | BlockFace::Up) {
        return face;
    }
    let order = [
        BlockFace::North,
        BlockFace::East,
        BlockFace::South,
        BlockFace::West,
    ];
    let index = order
        .iter()
        .position(|candidate| *candidate == face)
        .unwrap();
    order[(index + usize::from(turns % 4)) % 4]
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::RegistryProvenance;

    /// Creates a primary hopper state without unrelated registry capabilities.
    fn record(state: &str) -> RegistryRecord {
        RegistryRecord {
            beacon_tint: assets::BeaconTint::Unresolved,
            traits: assets::registry::BlockTraits::empty(),
            survival: assets::registry::SurvivalRule::None,
            properties: Box::new([]),
            sequential_id: 1,
            network_hash: 1,
            name: "minecraft:hopper".into(),
            canonical_state: state.into(),
            flags: BlockFlags::empty(),
            model_family: ModelFamily::Unknown,
            contributor_role: ContributorRole::Primary,
            model_state: Default::default(),
            face_coverage: 0,
            collision_seed: Default::default(),
            provenance: RegistryProvenance::PMMP,
        }
    }

    #[test]
    fn facing_accepts_the_toggle_bit_carried_by_hopper_states() {
        let record = record(
            r#"{"facing_direction":{"type":"int","value":4},"toggle_bit":{"type":"byte","value":0}}"#,
        );
        assert_eq!(facing(&record), Some(4));
    }

    #[test]
    fn hopper_outlet_follows_cardinal_facing() {
        for (facing, x, z) in [(2, 96..=160, 0..=64), (3, 96..=160, 192..=256)] {
            let quads = hopper_quads([0; 6], facing);
            assert_eq!(quads.len(), 32);
            assert!(quads.iter().any(|quad| {
                quad.positions
                    .iter()
                    .all(|[px, py, pz]| x.contains(px) && z.contains(pz) && (64..=128).contains(py))
            }));
        }
        for (facing, x, z) in [(4, 0..=64, 96..=160), (5, 192..=256, 96..=160)] {
            let quads = hopper_quads([0; 6], facing);
            assert_eq!(quads.len(), 32);
            assert!(quads.iter().any(|quad| {
                quad.positions
                    .iter()
                    .all(|[px, py, pz]| x.contains(px) && z.contains(pz) && (64..=128).contains(py))
            }));
        }
    }

    #[test]
    fn downward_hopper_has_a_stem_without_a_side_outlet() {
        let quads = hopper_quads([0; 6], 0);
        assert_eq!(quads.len(), 32);
        assert!(!quads.iter().any(|quad| {
            quad.positions.iter().all(|[x, y, z]| {
                (0..=64).contains(z) && (96..=160).contains(x) && (64..=128).contains(y)
            })
        }));
        assert!(quads.iter().any(|quad| {
            quad.positions
                .iter()
                .all(|[x, y, z]| (96..=160).contains(x) && (96..=160).contains(z) && *y <= 64)
        }));
    }
}
