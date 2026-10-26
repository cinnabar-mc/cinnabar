//! Face rebinding for plain full cubes with independent carried cube art.

use crate::{AssetError, BlockFlags, BlockVisual, Material, VisualKind};

/// Replaces named cube faces without moving models or their material groups.
pub(super) fn apply(
    replacements: &[(u32, [Option<u32>; 6])],
    visuals: &mut [BlockVisual],
    materials: &[Material],
    material_range: std::ops::Range<u32>,
) -> Result<(), AssetError> {
    let local_materials = material_range.end - material_range.start;
    let mut seen = std::collections::HashSet::new();
    for &(id, replacement) in replacements {
        if !seen.insert(id)
            || replacement
                .iter()
                .flatten()
                .any(|&id| id >= local_materials)
        {
            return Err(invalid(
                "base face override is duplicated or references an invalid material",
            ));
        }
        let visual = visuals
            .get_mut(id as usize)
            .ok_or_else(|| invalid("base face override visual is out of bounds"))?;
        if visual.kind != VisualKind::Cube
            || visual.model_template != crate::NO_MODEL_TEMPLATE
            || visual.variant != 0
            || visual.flags.contains(BlockFlags::LEAF_MODEL)
            || visual
                .faces
                .iter()
                .any(|&id| materials[id as usize].flags & crate::MATERIAL_FLAG_TINT_MASK != 0)
        {
            continue;
        }
        for (face, material) in replacement.iter().enumerate() {
            if let Some(material) = material {
                visual.faces[face] = material_range.start + material;
            }
        }
    }
    Ok(())
}

/// Names rejected bindings without exposing unchecked runtime references.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

/// Lists, in ascending order, the base visuals whose art differs between the base carrier
/// and an overlaid state: a rebound face, or a face or model-quad material, or one of its
/// weighted variations, that now samples another texture. Base templates and quads are
/// shared by both states.
pub(super) fn retextured_visuals(
    base: &super::RuntimeAssets,
    visuals: &[BlockVisual],
    materials: &[Material],
) -> Box<[u32]> {
    let art = |table: &[Material], id: u32| {
        let material = table.get(id as usize)?;
        let start = material.variation_start as usize;
        let leaves = table.get(start..start + material.variation_count as usize)?;
        Some((*material, leaves.to_vec()))
    };
    let changed = |id: u32| {
        id != crate::DIAGNOSTIC_MATERIAL && art(&base.materials, id) != art(materials, id)
    };
    let model_changed = |template: u32| {
        crate::model_template_parts(&base.model_templates, template)
            .into_iter()
            .flatten()
            .filter_map(|part| {
                let start = part.quad_start as usize;
                base.model_quads
                    .get(start..start + part.quad_count as usize)
            })
            .flatten()
            .any(|quad| changed(quad.material))
    };
    base.visuals
        .iter()
        .zip(visuals)
        .enumerate()
        .filter(|(_, (before, after))| {
            before.faces != after.faces
                || before.faces.iter().any(|&face| changed(face))
                || (before.model_template != crate::NO_MODEL_TEMPLATE
                    && model_changed(before.model_template))
        })
        .map(|(index, _)| index as u32)
        .collect()
}
