//! Chorus flowers are three crossed bars, drawn with withered art at their final age.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::vanilla_cuboid_quads;
use assets::facts::pack::PackBlock as B;

/// The age at which a flower can no longer grow and shows its withered art.
const WITHERED_AGE: u32 = 5;

pub(in crate::compiler) fn is_flower(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::ChorusFlower.identifier()
}

/// The terrain array entry a flower's age selects, for flowers only.
pub(in crate::compiler) fn flower_variant(record: &RegistryRecord) -> Option<usize> {
    if !is_flower(record) {
        return None;
    }
    let age = canonical_state_u32(
        &record.canonical_state,
        assets::facts::BlockProperty::Age.name(),
    )?;
    Some(usize::from(age == WITHERED_AGE))
}

/// Three full-length bars along each axis, each with one cross section.
fn flower_quads(materials: [u32; 6]) -> Vec<ModelQuad> {
    const LOW: i16 = 32;
    const HIGH: i16 = 224;
    (0..3)
        .flat_map(|axis| {
            let mut min = [LOW; 3];
            let mut max = [HIGH; 3];
            min[axis] = 0;
            max[axis] = 256;
            vanilla_cuboid_quads(materials, min, max)
        })
        .collect()
}

/// Compiles flowers, leaving packs without their art to the provisional shape.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_flower(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        let template = push_model_template(
            flower_quads(materials),
            assets::MODEL_TEMPLATE_FLAG_CHORUS,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(materials, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}
