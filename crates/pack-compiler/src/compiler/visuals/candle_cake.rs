//! Cake bases with one centered candle and its crossed wick.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockRenderKind as R, block_render_fact};

/// Identifies the undyed candle cake and the sixteen vanilla dye families.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::CandleCake
}

/// Validates the single typed lit selector and the cake collision contract.
fn lit(record: &RegistryRecord) -> Option<u8> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        lit: Tagged,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Tagged {
        #[serde(rename = "type")]
        kind: String,
        value: u8,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    (is_record(record)
        && state.lit.kind == "byte"
        && state.lit.value <= 1
        && record.model_family == ModelFamily::Unknown
        && record.model_state.mask() == 0
        && record.contributor_role == ContributorRole::Primary
        && record.flags.is_empty()
        && record.face_coverage == 0
        && super::cake::cake_collision_is_exact(record, 0))
    .then_some(state.lit.value)
}

/// Resolves the default cake and the corresponding candle's authored lit texture.
pub(in crate::compiler) fn descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<Vec<(Descriptor, Box<str>)>> {
    let lit = lit(record)?;
    let mut descriptors = super::cake::cake_material_descriptors(pack)?
        .into_iter()
        .collect::<Vec<_>>();
    let mut candle = record.clone();
    candle.name = assets::facts::pack::candle_companion(&record.name)?.into();
    descriptors.push(super::candle::descriptor(pack, &candle, lit != 0)?);
    Some(descriptors)
}

/// Compiles the composite before a collision-only cuboid can claim its identity.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 4], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    if lit(record).is_none() {
        return Ok(CompileRuleResult::Reject);
    }
    let mut visual = diagnostic_visual(record);
    let Some(descriptors) = descriptors(inputs.pack, record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(materials) = descriptors
        .into_iter()
        .map(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let [side, bottom, top, _, candle] = materials.as_slice() else {
        unreachable!("cake and candle descriptors");
    };
    let key = [*side, *bottom, *top, *candle];
    let faces = [*side, *side, *bottom, *top, *side, *side];
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template =
            push_model_template(quads(faces, *candle), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, faces, template);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits the cake, then one native candle centred on its top.
fn quads(faces: [u32; 6], candle: u32) -> Vec<ModelQuad> {
    let mut quads =
        super::geometry::vanilla_cuboid_quads(faces, [16, 0, 16], [240, 128, 240]).to_vec();
    quads.extend(super::candle::candle_quads(candle, [128, 128], 128, 6));
    quads
}
