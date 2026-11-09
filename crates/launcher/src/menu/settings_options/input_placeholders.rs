//! Server text names a control with `:_input_<binding>:`, such as `:_input_key.forward:`. The
//! client replaces each placeholder with the player's bound control for the current input mode.

use semantic_input::InputMode;

use super::{BindingId, SettingsOptions};

/// The marker that turns a `:name:` span into a binding placeholder.
const PLACEHOLDER_PREFIX: &str = "_input_";

impl SettingsOptions {
    /// Returns `text` with every `:_input_<binding>:` placeholder replaced by the control bound
    /// to `<binding>` in `mode`, or `None` when nothing was replaced.
    ///
    /// A known binding shows the same label as the controls screen; an unknown or unbound one
    /// shows `unassigned`. Touch reads the keyboard and mouse bindings.
    pub fn expand_input_placeholders(
        &self,
        text: &str,
        mode: InputMode,
        unassigned: &str,
    ) -> Option<String> {
        expand(text, |binding| {
            BindingId::from_name(binding)
                .and_then(|id| self.key_control(id.row(mode == InputMode::GamePad)?))
                .map(|control| self.key_name(control))
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| unassigned.to_owned())
        })
    }
}

/// Replaces placeholders left to right with `label(binding)`.
///
/// Each `:` opens a span that the next `:` closes. A span that is not a placeholder leaves its
/// closing `:` to open the next span. A replaced placeholder is not scanned again; scanning
/// resumes with the character after the replacement as the next opener.
fn expand(text: &str, mut label: impl FnMut(&str) -> String) -> Option<String> {
    let mut open = text.find(':')?;
    let mut text = std::borrow::Cow::Borrowed(text);
    loop {
        let start = open + 1;
        let Some(close) = text
            .as_bytes()
            .get(start..)
            .and_then(|rest| rest.iter().position(|&byte| byte == b':'))
            .map(|offset| start + offset)
        else {
            break;
        };
        // A span that starts with the ASCII prefix opens on a single-byte character and closes
        // on `:`, so slicing it by bytes stays on character boundaries.
        if text.as_bytes()[start..close].starts_with(PLACEHOLDER_PREFIX.as_bytes()) {
            let replacement = label(&text[start + PLACEHOLDER_PREFIX.len()..close]);
            text.to_mut().replace_range(open..=close, &replacement);
            open += replacement.len();
        } else {
            open = close;
        }
    }
    match text {
        std::borrow::Cow::Owned(text) => Some(text),
        std::borrow::Cow::Borrowed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use semantic_input::{InputMode, PhysicalControl};

    use super::{super::SettingsOptions, BindingId, expand};

    fn labels(text: &str) -> String {
        expand(text, |binding| format!("<{binding}>")).unwrap_or_else(|| text.to_owned())
    }

    #[test]
    fn replaces_placeholders_and_leaves_other_colon_spans() {
        assert_eq!(
            labels("Hold :_input_key.forward: to walk"),
            "Hold <key.forward> to walk"
        );
        assert_eq!(
            labels("Move with :_input_key.left:, :_input_key.right:"),
            "Move with <key.left>, <key.right>"
        );
        assert_eq!(labels("10:30 :smile: :_input_:"), "10:30 :smile: <>");
        assert_eq!(
            labels(":_gamepad_face_button_down:"),
            ":_gamepad_face_button_down:"
        );
        assert_eq!(labels("é:_input_key.jump:é"), "é<key.jump>é");
        assert_eq!(labels("no placeholder: here"), "no placeholder: here");
        assert_eq!(labels(":_input_key.jump"), ":_input_key.jump");
    }

    /// The character after a replacement opens the next span even when it is not a colon,
    /// and replacement text is never scanned again.
    #[test]
    fn resumes_after_the_replacement() {
        assert_eq!(
            labels(":_input_key.jump:x_input_key.use:"),
            "<key.jump><key.use>"
        );
        let expanded = expand(":_input_key.jump::_input_key.use:", |_| ":_input_a:".into());
        assert_eq!(expanded.as_deref(), Some(":_input_a::_input_a:"));
        assert_eq!(expand("plain text", |_| String::new()), None);
    }

    #[test]
    fn keyboard_placeholders_follow_the_players_bindings() {
        let mut settings = SettingsOptions::default();
        let hint = |settings: &SettingsOptions, mode| {
            settings.expand_input_placeholders(
                "Hold :_input_key.forward: to walk, :_input_key.attack: to hit, :_input_key.nope:",
                mode,
                "Unassigned",
            )
        };
        assert_eq!(
            hint(&settings, InputMode::KeyboardMouse).as_deref(),
            Some("Hold W to walk, Mouse 1 to hit, Unassigned")
        );
        let forward = BindingId::Forward.row(false).unwrap();
        assert!(settings.remap(forward, PhysicalControl::KeyboardUsage(0x0c)));
        assert_eq!(
            hint(&settings, InputMode::Touch).as_deref(),
            Some("Hold I to walk, Mouse 1 to hit, Unassigned")
        );
    }

    #[test]
    fn gamepad_placeholders_read_gamepad_rows() {
        let settings = SettingsOptions::default();
        let text = ":_input_key.jump: / :_input_key.forward:";
        assert_eq!(
            settings
                .expand_input_placeholders(text, InputMode::GamePad, "Unassigned")
                .as_deref(),
            Some("A / Unassigned")
        );
    }
}
