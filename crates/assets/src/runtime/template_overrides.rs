//! Template rebinding for base models and cubes redrawn with overlay geometry.

use crate::{
    AssetError, BlockFlags, BlockVisual, MODEL_TEMPLATE_FLAG_POTTED_PLANT, ModelQuad,
    ModelTemplate, VisualKind,
};

/// Points base model visuals at overlay templates. Each replacement must copy the base
/// template's complete selectable span (topology group or compound chain) with the same flags and
/// quad counts, and keep a pot-content template directly before it exactly when the base has one,
/// so neighbour-selected offsets and potted contents stay inside the copy. Plain cubes may
/// instead select six full-block model faces, losing cube occlusion.
pub(super) fn apply(
    replacements: &[(u32, u32)],
    visuals: &mut [BlockVisual],
    base_templates: &[ModelTemplate],
    overlay_templates: &[ModelTemplate],
    overlay_quads: &[ModelQuad],
    template_base: u32,
) -> Result<(), AssetError> {
    let mut seen = std::collections::HashSet::new();
    for &(id, replacement) in replacements {
        if !seen.insert(id) {
            return Err(invalid("base template override is duplicated"));
        }
        let visual = visuals
            .get_mut(id as usize)
            .ok_or_else(|| invalid("base template override visual is out of bounds"))?;
        if visual.kind == VisualKind::Cube
            && visual.model_template == crate::NO_MODEL_TEMPLATE
            && visual.variant == 0
            && !visual.flags.contains(BlockFlags::LEAF_MODEL)
        {
            let template = overlay_templates
                .get(replacement as usize)
                .ok_or_else(|| invalid("cube template override is out of bounds"))?;
            let quads = overlay_quads
                .get(template.quad_start as usize..)
                .and_then(|quads| quads.get(..6));
            if template.flags != 0
                || template.quad_count != 6
                || quads.is_none_or(|quads| {
                    quads.iter().enumerate().any(|(face, quad)| {
                        !crate::model::unit_cube_quad_geometry_is_valid(
                            face,
                            quad.positions,
                            quad.flags & crate::MODEL_QUAD_FLAG_FACE_MASK,
                        )
                    })
                })
            {
                return Err(invalid("cube template override is not a full-block model"));
            }
            visual.kind = VisualKind::Model;
            visual
                .flags
                .remove(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE);
            visual.model_template = template_base + replacement;
            continue;
        }
        if !matches!(visual.kind, VisualKind::Model | VisualKind::Cross)
            || visual.model_template == crate::NO_MODEL_TEMPLATE
        {
            return Err(invalid("base template override targets no model visual"));
        }
        let original = visual.model_template;
        let span = crate::model_template_span(base_templates, original)
            .ok_or_else(|| invalid("base template override targets an unbounded template"))?;
        let copied = overlay_templates
            .get(replacement as usize..)
            .and_then(|tail| tail.get(..span as usize));
        let source = &base_templates[original as usize..][..span as usize];
        let potted = |templates: &[ModelTemplate], index: u32| {
            index
                .checked_sub(1)
                .and_then(|previous| templates.get(previous as usize))
                .is_some_and(|template| template.flags == MODEL_TEMPLATE_FLAG_POTTED_PLANT)
        };
        if copied.is_none_or(|copied| {
            copied
                .iter()
                .zip(source)
                .any(|(copy, base)| copy.flags != base.flags || copy.quad_count != base.quad_count)
        }) || potted(base_templates, original) != potted(overlay_templates, replacement)
        {
            return Err(invalid(
                "base template override does not copy its template span",
            ));
        }
        visual.model_template = template_base + replacement;
    }
    Ok(())
}

/// Names rejected overrides without exposing unchecked runtime references.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
