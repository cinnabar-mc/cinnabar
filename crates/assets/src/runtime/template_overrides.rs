//! Template rebinding for base model visuals redrawn with overlay copies of their geometry.

use crate::{AssetError, BlockVisual, MODEL_TEMPLATE_FLAG_POTTED_PLANT, ModelTemplate, VisualKind};

/// Points base model visuals at overlay templates. Each replacement must copy the base
/// template's complete selectable span (topology group or compound chain) with the same flags and
/// quad counts, and keep a pot-content template directly before it exactly when the base has one,
/// so neighbour-selected offsets and potted contents stay inside the copy.
pub(super) fn apply(
    replacements: &[(u32, u32)],
    visuals: &mut [BlockVisual],
    base_templates: &[ModelTemplate],
    overlay_templates: &[ModelTemplate],
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
