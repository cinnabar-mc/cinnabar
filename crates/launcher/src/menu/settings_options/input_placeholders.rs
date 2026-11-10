//! Server text names controls with `:_input_<binding>:`, such as `:_input_key.forward:`, and
//! gamepad buttons with `:_gamepad_<button>:`. The client replaces each placeholder with the
//! player's bound control for the current input mode, then turns `:name:` emoticons into their
//! glyphs, as vanilla's `ClientInstance::emoticonifyText` does.

use std::borrow::Cow;

use assets::emoticons::EmoticonList;
use semantic_input::InputMode;

use super::{BindingId, SettingId, SettingsOptions, control_emoticon};

/// The marker that turns a `:name:` span into a binding placeholder.
const INPUT_PREFIX: &str = "_input_";
/// The marker that turns a `:name:` span into a gamepad button placeholder.
const GAMEPAD_PREFIX: &str = "_gamepad_";
/// The emoticon family a `:_gamepad_<button>:` placeholder becomes.
const GAMEPAD_EMOTICON_PREFIX: &str = "xbox_";
/// The face buttons the A/B and X/Y swap options exchange in gamepad placeholders.
const SWAPPED_FACE_BUTTONS: [[&str; 2]; 2] = [
    [":xbox_face_button_down:", ":xbox_face_button_right:"],
    [":xbox_face_button_left:", ":xbox_face_button_up:"],
];

/// Expanding the placeholders would make the text longer than the caller's byte limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpandedTextTooLarge;

/// One placeholder span, without its colons and marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Placeholder<'a> {
    Input(&'a str),
    Gamepad(&'a str),
}

impl SettingsOptions {
    /// Returns `text` with every `:_input_<binding>:` and `:_gamepad_<button>:` placeholder
    /// replaced and every emoticon in `emoticons` turned into its glyph, or `None` when nothing
    /// changed.
    ///
    /// A binding shows its control's emoticon when `emoticons` lists it: mouse buttons on
    /// keyboard and mouse, and every gamepad control in gamepad mode. Other controls, and
    /// controls without a listed emoticon, show the controls screen's label; an unknown or
    /// unbound binding shows `unassigned`. Touch reads the keyboard and mouse bindings and
    /// names them with text. `:_gamepad_<button>:` becomes the `:xbox_<button>:` emoticon in
    /// every input mode, after the A/B and X/Y swap options. The work is linear in the result,
    /// which never exceeds `max_bytes`.
    pub fn emoticonify_text(
        &self,
        text: &str,
        mode: InputMode,
        emoticons: &EmoticonList,
        unassigned: &str,
        max_bytes: usize,
    ) -> Result<Option<String>, ExpandedTextTooLarge> {
        let expanded = expand(text, max_bytes, |placeholder| match placeholder {
            Placeholder::Input(binding) => self.input_label(binding, mode, emoticons, unassigned),
            Placeholder::Gamepad(button) => Cow::Owned(self.gamepad_emoticon(button)),
        })?;
        let source = expanded.as_deref().unwrap_or(text);
        Ok(emoticons.emoticonify(source).or(expanded))
    }

    /// The text that replaces `:_input_<binding>:` in `mode`.
    fn input_label<'a>(
        &self,
        binding: &str,
        mode: InputMode,
        emoticons: &EmoticonList,
        unassigned: &'a str,
    ) -> Cow<'a, str> {
        let Some(control) = BindingId::from_name(binding)
            .and_then(|id| self.key_control(id.row(mode == InputMode::GamePad)?))
        else {
            return Cow::Borrowed(unassigned);
        };
        if mode != InputMode::Touch
            && let Some(name) =
                control_emoticon(control).filter(|name| emoticons.get(name).is_some())
        {
            return Cow::Owned(format!(":{name}:"));
        }
        let label = self.key_name(control);
        if label.is_empty() {
            Cow::Borrowed(unassigned)
        } else {
            Cow::Owned(label)
        }
    }

    /// The emoticon that replaces `:_gamepad_<button>:`, with the face buttons exchanged by
    /// the swap options.
    fn gamepad_emoticon(&self, button: &str) -> String {
        let mut emoticon = format!(":{GAMEPAD_EMOTICON_PREFIX}{button}:");
        let swaps = [
            SettingId::SwapGamepadAbButtons,
            SettingId::SwapGamepadXyButtons,
        ];
        for (pair, setting) in SWAPPED_FACE_BUTTONS.iter().zip(swaps) {
            if self.value(setting) == 0 {
                continue;
            }
            if let Some(index) = pair.iter().position(|name| *name == emoticon) {
                emoticon = pair[1 - index].to_owned();
                break;
            }
        }
        emoticon
    }
}

/// Replaces placeholders left to right with `label(placeholder)`, failing before the result
/// would exceed `max_bytes`.
///
/// Each `:` opens a span that the next `:` closes. A span that is not a placeholder leaves its
/// closing `:` to open the next span. A replaced placeholder is not scanned again; the
/// character after it opens the next span, even when it is not a `:`.
fn expand<'a>(
    text: &str,
    max_bytes: usize,
    mut label: impl FnMut(Placeholder<'_>) -> Cow<'a, str>,
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
        // A span that starts with an ASCII marker opens on a single-byte character and closes
        // on `:`, so slicing it by bytes stays on character boundaries.
        let span = &bytes[open + 1..close];
        let placeholder = if span.starts_with(INPUT_PREFIX.as_bytes()) {
            Some(Placeholder::Input(
                &text[open + 1 + INPUT_PREFIX.len()..close],
            ))
        } else if span.starts_with(GAMEPAD_PREFIX.as_bytes()) {
            Some(Placeholder::Gamepad(
                &text[open + 1 + GAMEPAD_PREFIX.len()..close],
            ))
        } else {
            None
        };
        let Some(placeholder) = placeholder else {
            open = close;
            continue;
        };
        let replacement = label(placeholder);
        if expanded.len() + (open - copied) + replacement.len() > max_bytes {
            return Err(ExpandedTextTooLarge);
        }
        expanded.push_str(&text[copied..open]);
        expanded.push_str(&replacement);
        copied = close + 1;
        open = close + 1;
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
    use assets::emoticons::EmoticonList;
    use semantic_input::{InputMode, PhysicalControl};

    use super::{
        super::{SettingId, SettingsOptions},
        BindingId, ExpandedTextTooLarge, Placeholder, expand,
    };

    fn labels(text: &str) -> String {
        expand(text, usize::MAX, |placeholder| {
            match placeholder {
                Placeholder::Input(binding) => format!("<{binding}>"),
                Placeholder::Gamepad(button) => format!("[{button}]"),
            }
            .into()
        })
        .unwrap()
        .unwrap_or_else(|| text.to_owned())
    }

    fn glyph(name: &str) -> char {
        EmoticonList::builtin().get(name).unwrap()
    }

    fn emoticonify(settings: &SettingsOptions, text: &str, mode: InputMode) -> String {
        settings
            .emoticonify_text(
                text,
                mode,
                EmoticonList::builtin(),
                "Unassigned",
                usize::MAX,
            )
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
            labels(":_gamepad_face_button_down: :_gamepad_:"),
            "[face_button_down] []"
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

    /// Keys keep their names; mouse buttons show their glyphs, as vanilla's mouse emoticons.
    #[test]
    fn keyboard_placeholders_follow_the_players_bindings() {
        let mut settings = SettingsOptions::default();
        let hint =
            "Hold :_input_key.forward: to walk, :_input_key.attack: to hit, :_input_key.nope:";
        let mouse = glyph("mouse_left_button");
        assert_eq!(
            emoticonify(&settings, hint, InputMode::KeyboardMouse),
            format!("Hold W to walk, {mouse} to hit, Unassigned")
        );
        let forward = BindingId::Forward.row(false).unwrap();
        assert!(settings.remap(forward, PhysicalControl::KeyboardUsage(0x0c)));
        assert_eq!(
            emoticonify(&settings, hint, InputMode::Touch),
            "Hold I to walk, Mouse 1 to hit, Unassigned"
        );
        let use_item = format!(
            ":_input_key.use: :_input_key.pickItem: {}",
            ":_input_key.inventory:"
        );
        assert_eq!(
            emoticonify(&settings, &use_item, InputMode::KeyboardMouse),
            format!(
                "{} {} E",
                glyph("mouse_right_button"),
                glyph("mouse_middle_button")
            )
        );
    }

    /// Without a listed emoticon, a control keeps its controls-screen label.
    #[test]
    fn controls_without_a_listed_emoticon_keep_their_names() {
        let settings = SettingsOptions::default();
        let text = ":_input_key.attack: / :_input_key.jump:";
        let none = EmoticonList::default();
        let named = |mode| settings.emoticonify_text(text, mode, &none, "Unassigned", usize::MAX);
        assert_eq!(
            named(InputMode::KeyboardMouse),
            Ok(Some("Mouse 1 / Space".to_owned()))
        );
        assert_eq!(named(InputMode::GamePad), Ok(Some("LT / A".to_owned())));
    }

    #[test]
    fn gamepad_placeholders_read_gamepad_rows() {
        let settings = SettingsOptions::default();
        let text = ":_input_key.jump: / :_input_key.forward:";
        assert_eq!(
            emoticonify(&settings, text, InputMode::GamePad),
            format!("{} / Unassigned", glyph("xbox_face_button_down"))
        );
    }

    /// `:_gamepad_<button>:` becomes the Xbox emoticon in every mode, after the face-button
    /// swap options; an unlisted button stays as its emoticon name.
    #[test]
    fn gamepad_buttons_become_their_emoticons() {
        let mut settings = SettingsOptions::default();
        let text = ":_gamepad_face_button_down: :_gamepad_face_button_left: :_gamepad_trigger_right: :_gamepad_nope:";
        let expected = |a: &str, x: &str| {
            format!(
                "{} {} {} :xbox_nope:",
                glyph(a),
                glyph(x),
                glyph("xbox_trigger_right")
            )
        };
        for mode in [
            InputMode::KeyboardMouse,
            InputMode::GamePad,
            InputMode::Touch,
        ] {
            assert_eq!(
                emoticonify(&settings, text, mode),
                expected("xbox_face_button_down", "xbox_face_button_left")
            );
        }
        assert!(settings.set(SettingId::SwapGamepadAbButtons.index(), 1));
        assert!(settings.set(SettingId::SwapGamepadXyButtons.index(), 1));
        assert_eq!(
            emoticonify(&settings, text, InputMode::KeyboardMouse),
            expected("xbox_face_button_right", "xbox_face_button_up")
        );
    }

    /// Emoticons the server writes itself turn into glyphs too, while raw placeholder output
    /// is not scanned for placeholders again.
    #[test]
    fn server_written_emoticons_become_glyphs() {
        let settings = SettingsOptions::default();
        assert_eq!(
            emoticonify(
                &settings,
                "Press :xbox_start: :heart:",
                InputMode::KeyboardMouse
            ),
            format!("Press {} :heart:", glyph("xbox_start"))
        );
        assert_eq!(
            settings.emoticonify_text(
                "no colons",
                InputMode::KeyboardMouse,
                EmoticonList::builtin(),
                "Unassigned",
                usize::MAX
            ),
            Ok(None)
        );
    }
}
