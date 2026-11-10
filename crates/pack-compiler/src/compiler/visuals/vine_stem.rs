//! Weeping and twisting vines draw stem art along their length and tip art where they end.

use super::context::{ModelStorage, RuleInputs, diagnostic_visual, push_model_template};
use super::cross::{PLANT_INSET, model_variant, plant_quads};
use super::dispatcher::CompileRuleResult;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

/// The template flag naming which neighbour continues this vine.
fn vine_flag(record: &RegistryRecord) -> Option<u32> {
    let name = record.name.as_ref();
    if name == B::WeepingVines.identifier() {
        Some(assets::MODEL_TEMPLATE_FLAG_WEEPING_VINE)
    } else if name == B::TwistingVines.identifier() {
        Some(assets::MODEL_TEMPLATE_FLAG_TWISTING_VINE)
    } else {
        None
    }
}

/// Stem planes on the `Up` face art and tip planes on the `Down` face art, in the order the
/// mesher's vine mask selects them.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 2], u32), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    let Some(flag) = vine_flag(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let mut visual = diagnostic_visual(record);
    if let (Some(stem), Some(tip), Some(variant)) = (
        inputs.material(record, BlockFace::Up),
        inputs.material(record, BlockFace::Down),
        model_variant(inputs.pack, record, BlockFace::Down),
    ) {
        let key = ([stem, tip], flag);
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let quads = [stem, tip]
                .into_iter()
                .flat_map(|material| plant_quads(material, 256, 0, PLANT_INSET))
                .collect();
            let template = push_model_template(quads, flag, storage.templates, storage.quads)?;
            templates.insert(key, template);
            template
        };
        visual.faces = [tip; 6];
        visual.kind = VisualKind::Cross;
        visual.model_template = template;
        visual.variant = variant;
    }
    Ok(CompileRuleResult::Compiled(visual))
}
