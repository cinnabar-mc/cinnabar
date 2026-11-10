use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

/// Whether the registry knows `record` only by network hash. The active packs may not
/// describe such a block, and the game draws those as full cubes of its missing tile.
pub(in crate::compiler) fn is_hash_only(record: &RegistryRecord) -> bool {
    record.name.as_ref() == assets::facts::block::RESERVED_BLOCK_NAME
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    let hash_only = is_hash_only(record);
    if (!hash_only && !record.flags.contains(BlockFlags::CUBE_GEOMETRY))
        || record_has_deferred_material(inputs.pack, record)
    {
        return CompileRuleResult::NoMatch;
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record) {
        visual.faces = materials;
        visual.kind = VisualKind::Cube;
        if hash_only {
            visual.flags |= BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
        }
    }
    CompileRuleResult::Compiled(visual)
}

#[cfg(test)]
#[path = "cube_tests.rs"]
mod tests;
