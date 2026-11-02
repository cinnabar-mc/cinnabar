use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    // Records known only by network hash are blocks the active packs may not describe, which
    // the game draws as full cubes of its missing tile.
    let hash_only = record.name.as_ref() == assets::facts::block::RESERVED_BLOCK_NAME;
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
