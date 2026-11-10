//! Observers, dispensers, droppers, crafters, piston bodies, jigsaws and vaults draw each
//! world face from the authored slot and UV flip their state selects
//! (`pack_compile_input::mapped_face`).
//!
//! A piston body stops a quarter short of its head, which the block entity renderer draws.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::block_render_fact;
use pack_compile_input::mapped_face::{self, Family};
use {super::super::*, assets::BlockFace};

/// Admits the block classes that map their faces by state.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    mapped_face::family(record).is_some()
}

/// Resolves the authored slot and class-selected terrain entry that world `face` draws.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let source_face = mapped_face::mapping(record)?.source_face(face);
    let variant = mapped_face::texture_variant(record)?;
    let key = resolve_texture_key(&pack.blocks, record, source_face).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    let mut flags = pack.blocks.isotropic_face_flags(record)[source_face as usize];
    if block_render_fact(&record.name).kind.route().cutout {
        flags |= MATERIAL_FLAG_ALPHA_CUTOUT;
    }
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: Some((state_variant, state_variant)),
            flags,
        },
        key,
    ))
}

/// What a mapped-face family draws beyond the plain flipped cube.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) enum Shape {
    Cube,
    Vault,
    PistonBody { facing: u8 },
}

/// A piston body's bounds for each `facing_direction`, with the quarter toward the head removed.
const PISTON_BODIES: [([i16; 3], [i16; 3]); 6] = [
    ([0, 64, 0], [256, 256, 256]),
    ([0, 0, 0], [256, 192, 256]),
    ([0, 0, 0], [256, 256, 192]),
    ([0, 0, 64], [256, 256, 256]),
    ([0, 0, 0], [192, 256, 256]),
    ([64, 0, 0], [256, 256, 256]),
];

/// The face toward the head for each `facing_direction`, matching the native piston directions.
const PISTON_HEAD_FACES: [BlockFace; 6] = [
    BlockFace::Down,
    BlockFace::Up,
    BlockFace::South,
    BlockFace::North,
    BlockFace::East,
    BlockFace::West,
];

/// The art corner a piston body's `face` samples at `[x, y, z]`, for a native UV rotation. A
/// side's art rows run away from the head, so a cropped body never samples the head's rows.
fn piston_uv(face: BlockFace, rotation: u8, [x, y, z]: [i16; 3]) -> [u16; 2] {
    let near = |coordinate: i16| coordinate as u16 * 16;
    let far = |coordinate: i16| 4096 - near(coordinate);
    match (face, rotation) {
        (BlockFace::Down | BlockFace::Up, 1) => [near(z), far(x)],
        (BlockFace::Down | BlockFace::Up, 2) => [far(z), near(x)],
        (BlockFace::Down | BlockFace::Up, 3) => [far(x), far(z)],
        (BlockFace::Down | BlockFace::Up, _) => [near(x), near(z)],
        (BlockFace::North, 1) => [near(y), far(x)],
        (BlockFace::North, 2) => [far(y), near(x)],
        (BlockFace::North, 3) => [near(x), near(y)],
        (BlockFace::North, _) => [far(x), far(y)],
        (BlockFace::South, 1) => [far(y), far(x)],
        (BlockFace::South, 2) => [near(y), near(x)],
        (BlockFace::South, 3) => [far(x), near(y)],
        (BlockFace::South, _) => [near(x), far(y)],
        (BlockFace::West, 1) => [far(y), far(z)],
        (BlockFace::West, 2) => [near(y), near(z)],
        (BlockFace::West, 3) => [far(z), near(y)],
        (BlockFace::West, _) => [near(z), far(y)],
        (BlockFace::East, 1) => [near(y), far(z)],
        (BlockFace::East, 2) => [far(y), near(z)],
        (BlockFace::East, 3) => [near(z), near(y)],
        (BlockFace::East, _) => [far(z), far(y)],
    }
}

/// A piston body's faces with native UV rotations; only the face toward the head stays visible
/// beside a neighbour, since it sits inside the cell.
fn piston_body_quads(materials: [u32; 6], flips: [u8; 6], facing: u8) -> [ModelQuad; 6] {
    let (min, max) = PISTON_BODIES[usize::from(facing)];
    let mut quads = super::geometry::cuboid_quads(materials, min, max);
    let head = PISTON_HEAD_FACES[usize::from(facing)];
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        quad.uvs = quad
            .positions
            .map(|corner| piston_uv(face, flips[face as usize], corner));
        if face != head {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    quads
}

/// Emits a full cube with the state's UV flips, plus the vault's interior; opaque cubes
/// occlude their neighbours. Malformed states stay diagnostic.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], [u8; 6], Shape), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some(family) = mapped_face::family(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let Some(mapping) = mapped_face::mapping(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let flips = BlockFace::ALL.map(|face| mapping.flip(face));
    let shape = match family {
        Family::Vault => Shape::Vault,
        Family::PistonBase => {
            let Some(facing) = mapped_face::piston_facing(record) else {
                return Ok(CompileRuleResult::Reject);
            };
            Shape::PistonBody { facing }
        }
        _ => Shape::Cube,
    };
    let key = (materials, flips, shape);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads = super::geometry::flipped_cube_quads(materials, flips).to_vec();
        let template = if let Shape::PistonBody { facing } = shape {
            let template = push_model_template(
                piston_body_quads(materials, flips, facing).to_vec(),
                assets::MODEL_TEMPLATE_FLAG_PISTON_BODY,
                storage.templates,
                storage.quads,
            )?;
            push_model_template(quads, 0, storage.templates, storage.quads)?;
            template
        } else {
            if shape == Shape::Vault {
                quads.extend(super::vault::inner_quads(materials, flips));
            }
            push_model_template(quads, 0, storage.templates, storage.quads)?
        };
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    // An extended body leaves its head side open, so a piston never hides its neighbours' faces.
    if !block_render_fact(&record.name).kind.route().cutout && family != Family::PistonBase {
        visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
    }
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One facing's native flips, in the order [`compile_rule`] passes them.
    fn flips(facing: usize) -> [u8; 6] {
        BlockFace::ALL.map(|face| mapped_face::PISTON_FLIPS[facing][face.facing_index()])
    }

    fn cull(quad: &ModelQuad) -> u32 {
        quad.flags & (MODEL_QUAD_FLAG_FACE_MASK << 4)
    }

    /// The art row a corner of `quad` samples, in UV units.
    fn art_rows(quad: &ModelQuad) -> impl Iterator<Item = u16> + '_ {
        quad.uvs.iter().map(|uv| uv[1])
    }

    #[test]
    fn a_piston_body_is_three_quarters_and_leaves_its_head_side_uncullable() {
        for facing in 0..6u8 {
            let quads = piston_body_quads([1, 2, 3, 4, 5, 6], flips(usize::from(facing)), facing);
            let head = PISTON_HEAD_FACES[usize::from(facing)];
            let axis = usize::from(facing / 2);
            let axis = [1, 2, 0][axis];
            let (min, max) = PISTON_BODIES[usize::from(facing)];
            assert_eq!(max[axis] - min[axis], 192, "{facing}");
            for (face, quad) in BlockFace::ALL.into_iter().zip(&quads) {
                assert_eq!(cull(quad) == 0, face == head, "{facing} {face:?}");
            }
        }
    }

    #[test]
    fn piston_sides_turn_their_art_like_a_flipped_full_cube() {
        for flip in 0..4u8 {
            let cube = super::super::geometry::flipped_cube_quads([1, 2, 3, 4, 5, 6], [flip; 6]);
            for (face, quad) in BlockFace::ALL.into_iter().zip(&cube) {
                if matches!(face, BlockFace::Down | BlockFace::Up) {
                    continue;
                }
                for (corner, uv) in quad.positions.iter().zip(&quad.uvs) {
                    assert_eq!(piston_uv(face, flip, *corner), *uv, "{face:?} {flip}");
                }
            }
        }
    }

    #[test]
    fn a_piston_body_side_never_samples_the_heads_art_rows() {
        for facing in 0..6usize {
            let quads = piston_body_quads([1, 2, 3, 4, 5, 6], flips(facing), facing as u8);
            let head = PISTON_HEAD_FACES[facing];
            let opposite = PISTON_HEAD_FACES[facing ^ 1];
            for (face, quad) in BlockFace::ALL.into_iter().zip(&quads) {
                if face == head || face == opposite {
                    continue;
                }
                assert!(
                    art_rows(quad).all(|row| row >= 1024),
                    "{facing} {face:?} {:?}",
                    quad.uvs
                );
            }
        }
    }
}
