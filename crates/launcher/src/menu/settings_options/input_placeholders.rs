//! Server text names a control with `:_input_<binding>:`, such as `:_input_key.forward:`. The
//! client replaces each placeholder with the player's bound control for the current input mode.

use std::borrow::Cow;

use semantic_input::InputMode;

use super::{BindingId, SettingsOptions};

/// The marker that turns a `:name:` span into a binding placeholder.
const PLACEHOLDER_PREFIX: &str = "_input_";

/// Expanding the placeholders would make the text longer than the caller's byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpandedTextTooLarge;

impl SettingsOptions {
    /// Returns `text` with every `:_input_<binding>:` placeholder replaced by the control bound
    /// to `<binding>` in `mode`, or `None` when nothing was replaced.
    ///
    /// A known binding shows the same label as the controls screen; an unknown or unbound one
    /// shows `unassigned`. Touch reads the keyboard and mouse bindings. The work is linear in
    /// the result, which never exceeds `max_bytes`.
    pub fn expand_input_placeholders(
        &self,
        text: &str,
        mode: InputMode,
        unassigned: &str,
        max_bytes: usize,
    ) -> Result<Option<String>, ExpandedTextTooLarge> {
        expand(text, max_bytes, |binding| {
            BindingId::from_name(binding)
                .and_then(|id| self.key_control(id.row(mode == InputMode::GamePad)?))
                .map(|control| self.key_name(control))
                .filter(|label| !label.is_empty())
                .map_or(Cow::Borrowed(unassigned), Cow::Owned)
        })
    }
}

/// Replaces placeholders left to right with `label(binding)`, failing before the result
/// would exceed `max_bytes`.
///
/// Each `:` opens a span that the next `:` closes. A span that is not a placeholder leaves its
/// closing `:` to open the next span. A replaced placeholder is not scanned again; the
/// character after it opens the next span, even when it is not a `:`.
fn expand<'a>(
    text: &str,
    max_bytes: usize,
    mut label: impl FnMut(&str) -> Cow<'a, str>,
) -> Result<Option<String>, ExpandedTextTooLarge> {
    let Some(mut open) = text.find(':') else {
        return Ok(None);
    };
    let bytes = text.as_bytes();
    let mut expanded = String::new();
    let mut copied = 0;
    while let Some(close) = bytes
        .get(open + 1..)
        .and_then(|rest| rest.iter().position(|&byte| byte == b':'))
        .map(|offset| open + 1 + offset)
    {
        // A span that starts with the ASCII prefix opens on a single-byte character and closes
        // on `:`, so slicing it by bytes stays on character boundaries.
        if bytes[open + 1..close].starts_with(PLACEHOLDER_PREFIX.as_bytes()) {
            let replacement = label(&text[open + 1 + PLACEHOLDER_PREFIX.len()..close]);
            if expanded.len() + (open - copied) + replacement.len() > max_bytes {
                return Err(ExpandedTextTooLarge);
            }
            expanded.push_str(&text[copied..open]);
            expanded.push_str(&replacement);
            copied = close + 1;
            open = close + 1;
        } else {
            open = close;
        }
    }
    if copied == 0 {
        return Ok(None);
    }
    if expanded.len() + (text.len() - copied) > max_bytes {
        return Err(ExpandedTextTooLarge);
    }
    expanded.push_str(&text[copied..]);
    Ok(Some(expanded))
}

#[cfg(test)]
mod tests {
    use semantic_input::{InputMode, PhysicalControl};

    use super::{super::SettingsOptions, BindingId, ExpandedTextTooLarge, expand};

    fn labels(text: &str) -> String {
        expand(text, usize::MAX, |binding| format!("<{binding}>").into())
            .unwrap()
            .unwrap_or_else(|| text.to_owned())
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
        let expanded = expand(":_input_key.jump::_input_key.use:", usize::MAX, |_| {
            ":_input_a:".into()
        });
        assert_eq!(expanded, Ok(Some(":_input_a::_input_a:".to_owned())));
        assert_eq!(expand("plain text", usize::MAX, |_| "".into()), Ok(None));
    }

    /// Many placeholders with a long label stop at the limit instead of building the result.
    #[test]
    fn refuses_results_over_the_byte_limit() {
        let label = "x".repeat(1024);
        let text = ":_input_:".repeat(1000);
        let mut calls = 0;
        let result = expand(&text, 64 * 1024, |_| {
            calls += 1;
            label.as_str().into()
        });
        assert_eq!(result, Err(ExpandedTextTooLarge));
        assert!(calls <= 65);
        let fits = expand(":_input_: tail", 7, |_| "ab".into());
        assert_eq!(fits, Ok(Some("ab tail".to_owned())));
        assert_eq!(
            expand(":_input_: tail", 7, |_| "abc".into()),
            Err(ExpandedTextTooLarge)
        );
    }

    #[test]
    fn keyboard_placeholders_follow_the_players_bindings() {
        let mut settings = SettingsOptions::default();
        let hint = |settings: &SettingsOptions, mode| {
            settings.expand_input_placeholders(
                "Hold :_input_key.forward: to walk, :_input_key.attack: to hit, :_input_key.nope:",
                mode,
                "Unassigned",
                usize::MAX,
            )
        };
        assert_eq!(
            hint(&settings, InputMode::KeyboardMouse),
            Ok(Some(
                "Hold W to walk, Mouse 1 to hit, Unassigned".to_owned()
            ))
        );
        let forward = BindingId::Forward.row(false).unwrap();
        assert!(settings.remap(forward, PhysicalControl::KeyboardUsage(0x0c)));
        assert_eq!(
            hint(&settings, InputMode::Touch),
            Ok(Some(
                "Hold I to walk, Mouse 1 to hit, Unassigned".to_owned()
            ))
        );
    }

    #[test]
    fn gamepad_placeholders_read_gamepad_rows() {
        let settings = SettingsOptions::default();
        let text = ":_input_key.jump: / :_input_key.forward:";
        assert_eq!(
            settings.expand_input_placeholders(text, InputMode::GamePad, "Unassigned", usize::MAX),
            Ok(Some("A / Unassigned".to_owned()))
        );
    }
}
