//! Pointed dripstone and sulfur spikes draw as one crossed sprite chosen by thickness and direction.

use super::context::{ModelStorage, RuleInputs, diagnostic_visual, push_model_template};
use super::cross::{PLANT_INSET, plant_quads};
use super::dispatcher::CompileRuleResult;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    let name = record.name.as_ref();
    name == B::PointedDripstone.identifier() || name == B::SulfurSpike.identifier()
}

/// The block face whose art each thickness draws, in the thickness property's domain order: tip,
/// frustum, middle, base and merge.
const THICKNESS_FACES: [BlockFace; 5] = [
    BlockFace::North,
    BlockFace::Down,
    BlockFace::South,
    BlockFace::Up,
    BlockFace::West,
];

/// Reads the block face whose art a thickness draws and its terrain array entry: stalactite art
/// comes first, then stalagmite art.
fn state(record: &RegistryRecord) -> Option<(BlockFace, usize)> {
    use assets::facts::{BlockProperty, PropertyDomain};
    let PropertyDomain::String(thicknesses) = BlockProperty::DripstoneThickness.domain() else {
        return None;
    };
    let thickness = canonical_state_str(
        &record.canonical_state,
        BlockProperty::DripstoneThickness.name(),
    )?;
    let index = thicknesses
        .iter()
        .position(|value| *value == thickness.as_ref())?;
    let face = *THICKNESS_FACES.get(index)?;
    let hanging = canonical_state_u32(&record.canonical_state, BlockProperty::Hanging.name())? != 0;
    Some((face, usize::from(!hanging)))
}

/// Resolves the one sprite a state draws on every face.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<(Descriptor, Box<str>)> {
    let (face, variant) = state(record)?;
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<u32, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(material) = inputs.material(record, BlockFace::Up) {
        let template = if let Some(&template) = templates.get(&material) {
            template
        } else {
            let template = push_model_template(
                plant_quads(material, 256, 0, PLANT_INSET).to_vec(),
                assets::MODEL_TEMPLATE_FLAG_SPELEOTHEM,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(material, template);
            template
        };
        visual.faces = [material; 6];
        visual.kind = VisualKind::Cross;
        visual.model_template = template;
    }
    Ok(CompileRuleResult::Compiled(visual))
}
