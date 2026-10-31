//! Azalea bushes cross a stem through a canopy shell with no floor.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::cross::{PLANT_INSET, plant_quads};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// The crossed stem art, then the canopy's top and four sides, each culled by its own neighbour.
/// Every quad is two-sided like the crossed foliage whose render layer the bush shares.
fn azalea_quads(stem: u32, side: u32, top: u32) -> Vec<ModelQuad> {
    let mut faces = [side; 6];
    faces[BlockFace::Up as usize] = top;
    let mut quads = plant_quads(stem, 256, 0, PLANT_INSET).to_vec();
    for (face, mut quad) in BlockFace::ALL
        .into_iter()
        .zip(vanilla_cuboid_quads(faces, [0; 3], [256; 3]))
    {
        if face != BlockFace::Down {
            quad.flags |=
                MODEL_QUAD_FLAG_TWO_SIDED | ((quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4);
            quads.push(quad);
        }
    }
    quads
}

/// Compiles the bush from its stem, canopy-side and canopy-top art, leaving packs without that
/// art to the provisional shape.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if block_render_fact(&record.name).kind != R::Azalea {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key =
        [BlockFace::East, BlockFace::North, BlockFace::Up].map(|face| materials[face as usize]);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let [stem, side, top] = key;
        let template = push_model_template(
            azalea_quads(stem, side, top),
            assets::MODEL_TEMPLATE_FLAG_AZALEA,
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
