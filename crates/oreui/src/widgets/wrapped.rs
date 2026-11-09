//! Raised buttons whose labels remain readable when a narrow layout needs more lines.

use super::{Canvas, MenuAction, MenuView, PaintError, Variant};
use crate::{paint::Bounds, theme};

const INLINE_PADDING: f32 = 1.6;
const BLOCK_PADDING: f32 = 0.6;

/// Fits a label to the available width and reserves every wrapped line above the shadow.
pub(in super::super) fn measure(
    canvas: &mut Canvas<'_>,
    available: f32,
    variant: Variant,
    label: &str,
) -> Result<[f32; 2], PaintError> {
    let padding = 2.0 * canvas.r(INLINE_PADDING);
    let edge = 2.0 * canvas.r(theme::EDGE);
    let width = (canvas.measure(label, variant.label())? + padding + edge).min(available.max(1.0));
    let text = canvas.measure_height(label, (width - padding - edge).max(1.0), variant.label())?;
    let height = (text + 2.0 * canvas.r(BLOCK_PADDING) + canvas.r(theme::BUTTON_DEPTH) + edge)
        .max(canvas.r(theme::BUTTON_HEIGHT));
    Ok([width, height])
}

/// Reuses the normal button face, focus and hit target, centering the complete wrapped label.
pub(in super::super) fn button(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    variant: Variant,
    label: &str,
    action: Option<MenuAction>,
) -> Result<(), PaintError> {
    let state = canvas.interaction(view, action);
    let (face, color) = super::button_art(canvas, bounds, variant, state, action.is_some())?;
    let padding = canvas.r(INLINE_PADDING);
    let width = (face[2] - face[0] - 2.0 * padding).max(1.0);
    let height = canvas.measure_height(label, width, variant.label())?;
    canvas.centered_wrapped_text_with_shadow(
        label,
        [face[0] + padding, (face[1] + face[3] - height) * 0.5],
        width,
        variant.label(),
        color,
        matches!(variant, Variant::Hero) && action.is_some(),
    )?;
    if let Some(action) = action {
        canvas.hit(action, bounds)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn wrapped_button_keeps_the_whole_label_inside_its_hit_target() {
        let action = MenuAction::Mods(launcher::mods::Action::BrowseMarketplace);
        let mut view = MenuView::new(true, "Player".into());
        view.focused_action = Some(action);
        view.navigation_focus_visible = true;
        let (_, hits, nodes) = crate::review_tests::paint(HashMap::new(), |canvas| {
            let [width, height] =
                measure(canvas, 120.0, Variant::Primary, "Browse marketplace").unwrap();
            button(
                canvas,
                &view,
                [20.0, 20.0, 20.0 + width, 20.0 + height],
                Variant::Primary,
                "Browse marketplace",
                Some(action),
            )
            .unwrap();
        });
        let bounds = hits.iter().find(|(hit, _)| *hit == action).unwrap().1;
        let mut visible = String::new();
        for node in &nodes {
            if let ui::UiVisual::Text { layout, .. } = node.visual() {
                assert!(bounds.contains(node.bounds().min()));
                assert!(bounds.contains(node.bounds().max()));
                visible.extend(
                    layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .filter(|character| !character.is_whitespace()),
                );
            }
        }
        assert_eq!(visible, "Browsemarketplace");
    }
}
