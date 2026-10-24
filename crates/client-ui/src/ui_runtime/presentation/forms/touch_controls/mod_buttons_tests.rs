use super::*;
use ui::UiVisual;

fn button(id: &str, label: &str) -> TouchButton {
    TouchButton {
        id: id.into(),
        label: label.into(),
        anchor: Anchor::TopRight,
        offset: [4., 4.],
        size: [48., 18.],
    }
}

/// Paints the gameplay touch overlay and returns the largest painted text run, in glyphs.
fn widest_text(presentation: &mut UiPresentationRuntime) -> usize {
    let mut nodes = Vec::new();
    presentation
        .append_touch_controls(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 0,
            TextMetrics::for_viewport([854, 480], ui::DpiScale::new(1.).unwrap(), None),
            [854., 480.],
        )
        .unwrap();
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(layout.glyphs().len()),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Labels that look like layout expressions still paint as their literal text.
#[test]
fn labels_paint_literally() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.enable_touch_controls(true);
    presentation.set_mod_touch_buttons([button("demo", "(1 + 1)")].iter());
    assert!(widest_text(&mut presentation) >= "(1+1)".len());
}

/// Only a change of slot identity advances the epoch that cancels held fingers.
#[test]
fn identity_changes_advance_the_epoch() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.set_mod_touch_buttons([button("a", "Menu")].iter());
    let epoch = presentation.mod_touch_epoch();
    presentation.set_mod_touch_buttons([button("a", "Open")].iter());
    assert_eq!(presentation.mod_touch_epoch(), epoch);
    presentation.set_mod_touch_buttons([button("b", "Open")].iter());
    assert_ne!(presentation.mod_touch_epoch(), epoch);
    let epoch = presentation.mod_touch_epoch();
    presentation.set_mod_touch_buttons(std::iter::empty());
    assert_ne!(presentation.mod_touch_epoch(), epoch);
}

/// A tap resolves through the presented slot to its button id.
#[test]
fn taps_resolve_to_presented_button_ids() {
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.set_mod_touch_buttons([button("a", "A"), button("b", "B")].iter());
    presentation.complete_mod_touch_tap(touch::mod_button(1).unwrap());
    presentation.complete_mod_touch_tap(touch::JUMP);
    assert_eq!(presentation.take_mod_touch_taps(), ["b"]);
    assert!(presentation.take_mod_touch_taps().is_empty());
}
