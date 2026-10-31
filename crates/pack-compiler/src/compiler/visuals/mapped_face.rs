//! Observers, dispensers, droppers, crafters, piston bodies, jigsaws and vaults draw each
//! world face from the authored slot and UV flip their state selects
//! (`pack_compile_input::mapped_face`).

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

/// Emits a full cube with the state's UV flips, plus the vault's interior; opaque cubes
/// occlude their neighbours. Malformed states stay diagnostic.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], [u8; 6], bool), u32>,
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
    let interior = family == Family::Vault;
    let key = (materials, flips, interior);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let mut quads = super::geometry::flipped_cube_quads(materials, flips).to_vec();
        if interior {
            quads.extend(super::vault::inner_quads(materials, flips));
        }
        let template = push_model_template(quads, 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    if !block_render_fact(&record.name).kind.route().cutout {
        visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
    }
    // Piston bodies keep the fallback grade until the native three-quarter body and head exist.
    visual.support = if family == Family::PistonBase {
        VisualSupport::VanillaFallback
    } else {
        VisualSupport::Exact
    };
    Ok(CompileRuleResult::Compiled(visual))
}
