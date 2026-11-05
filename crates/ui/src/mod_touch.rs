//! Mod-registered touch buttons, drawn in the built-in touch control style.

use crate::mod_hud::Anchor;

/// Buttons one mod may retain at once.
pub const MAX_TOUCH_BUTTONS: usize = 4;
/// Buttons presented across every loaded mod; later mods' extra buttons are not shown.
pub const MAX_PRESENTED_TOUCH_BUTTONS: usize =
    (semantic_input::touch::MOD_BUTTONS.end - semantic_input::touch::MOD_BUTTONS.start) as usize;
pub const MAX_TOUCH_BUTTON_ID_BYTES: usize = 32;
pub const MAX_TOUCH_LABEL_BYTES: usize = 24;
/// Inclusive bounds of each button side, in GUI pixels.
pub const TOUCH_BUTTON_SIZE: [f32; 2] = [18., 96.];
/// Largest default offset from the anchored edge or centre, in GUI pixels.
pub const MAX_TOUCH_BUTTON_OFFSET: f32 = 1024.;
/// Taps retained for one mod until its next frame callback; later taps are dropped.
pub const MAX_QUEUED_TOUCH_PRESSES: usize = 16;
/// Saved-layout and editor names of mod buttons carry this prefix before the button id.
const PLACEMENT_PREFIX: &str = "mod-";

/// One button: a stable id, a centred one-line label and its default placement.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchButton {
    pub id: String,
    pub label: String,
    pub anchor: Anchor,
    /// GUI pixels inward from the anchored edges, or right/down from centred axes.
    pub offset: [f32; 2],
    /// Width and height in GUI pixels.
    pub size: [f32; 2],
}

/// Accepts 1–32 bytes of lowercase ASCII letters, digits, `_` and `-`.
pub fn valid_button_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_TOUCH_BUTTON_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
}

/// Accepts nonblank plain single-line text, which the layout editor also shows as a caption.
pub fn valid_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= MAX_TOUCH_LABEL_BYTES
        && crate::mod_hud::plain_text_valid(label)
}

/// Checks a mod's complete button set, including unique ids.
pub fn validate(buttons: &[TouchButton]) -> Result<(), String> {
    if buttons.len() > MAX_TOUCH_BUTTONS {
        return Err(format!("at most {MAX_TOUCH_BUTTONS} touch buttons"));
    }
    for (index, button) in buttons.iter().enumerate() {
        if !valid_button_id(&button.id) {
            return Err("touch button ids are 1-32 bytes of a-z, 0-9, _ and -".into());
        }
        if buttons[..index].iter().any(|other| other.id == button.id) {
            return Err(format!("duplicate touch button id {}", button.id));
        }
        if !valid_label(&button.label) {
            return Err(format!(
                "touch button labels are one line of 1-{MAX_TOUCH_LABEL_BYTES} bytes"
            ));
        }
        if !button
            .offset
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_TOUCH_BUTTON_OFFSET)
            || !button
                .size
                .iter()
                .all(|v| (TOUCH_BUTTON_SIZE[0]..=TOUCH_BUTTON_SIZE[1]).contains(v))
        {
            return Err("touch button placement or size is out of range".into());
        }
    }
    Ok(())
}

/// The saved-layout and editor name of a mod button, shared by every mod using its id.
pub fn placement_name(id: &str) -> String {
    format!("{PLACEMENT_PREFIX}{id}")
}

/// Whether a saved-layout name belongs to a mod button rather than an authored control.
pub fn is_placement_name(name: &str) -> bool {
    name.strip_prefix(PLACEMENT_PREFIX)
        .is_some_and(valid_button_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(id: &str) -> TouchButton {
        TouchButton {
            id: id.into(),
            label: "Menu".into(),
            anchor: Anchor::TopRight,
            offset: [4., 4.],
            size: [32., 18.],
        }
    }

    #[test]
    fn accepts_bounded_sets_and_rejects_invalid_buttons() {
        assert_eq!(
            validate(&[button("client-menu"), button("toggle_2")]),
            Ok(())
        );
        assert!(validate(&vec![button("a"); 1]).is_ok());
        let mut literal = button("literal");
        literal.label = "#(1 + 1)".into();
        assert_eq!(validate(&[literal]), Ok(()));
        let too_many: Vec<_> = (0..=MAX_TOUCH_BUTTONS)
            .map(|index| button(&format!("b{index}")))
            .collect();
        assert!(validate(&too_many).is_err());
        assert!(validate(&[button("a"), button("a")]).is_err());
        for id in [
            "",
            "Upper",
            "dot.ted",
            &"x".repeat(MAX_TOUCH_BUTTON_ID_BYTES + 1),
        ] {
            assert!(validate(&[button(id)]).is_err(), "{id:?}");
        }
        for label in ["", "  ", "§cRed", "two\nlines", &"x".repeat(25)] {
            let mut invalid = button("a");
            invalid.label = label.into();
            assert!(validate(&[invalid]).is_err(), "{label:?}");
        }
        for (offset, size) in [
            ([f32::NAN, 0.], [18., 18.]),
            ([2000., 0.], [18., 18.]),
            ([0., 0.], [17., 18.]),
            ([0., 0.], [18., 97.]),
        ] {
            let mut invalid = button("a");
            invalid.offset = offset;
            invalid.size = size;
            assert!(validate(&[invalid]).is_err());
        }
    }

    #[test]
    fn placement_names_round_trip_only_for_valid_ids() {
        assert!(is_placement_name(&placement_name("client-menu")));
        assert!(!is_placement_name("jump"));
        assert!(!is_placement_name("mod-"));
        assert!(!is_placement_name("mod-Bad"));
    }
}
