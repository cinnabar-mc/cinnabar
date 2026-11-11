//! The launcher and container text boxes' caret glyph, after vanilla's text edit component as the engine
//! models it: the `CARET_GLYPH` at the caret while the blink shows it.

use std::borrow::Cow;

use launcher::menu::{MenuCaret, MenuField, MenuView};
use ui_engine::caret::caret_byte;

/// `text` with the caret glyph at the caret while `field` is focused and the blink shows it.
pub fn with_caret<'a>(view: &MenuView, field: MenuField, text: &'a str) -> Cow<'a, str> {
    if view.field != Some(field) {
        return Cow::Borrowed(text);
    }
    insert_caret(&view.caret, text)
}

/// `text` with the caret glyph at `caret` while the blink shows it and nothing is selected.
pub fn insert_caret<'a>(caret: &MenuCaret, text: &'a str) -> Cow<'a, str> {
    if !caret.shown || caret.selection.is_some() {
        return Cow::Borrowed(text);
    }
    // Inside a formatting code the caret glyph would pair with its `§` and vanish.
    let at = ui::formatting_code_boundary(text, caret_byte(text, caret.byte));
    let mut shown = String::with_capacity(text.len() + json_ui::CARET_GLYPH.len_utf8());
    shown.push_str(&text[..at]);
    shown.push(json_ui::CARET_GLYPH);
    shown.push_str(&text[at..]);
    Cow::Owned(shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_glyph_keeps_formatting_codes_intact() {
        let caret = MenuCaret {
            byte: '§'.len_utf8(),
            shown: true,
            ..Default::default()
        };
        let shown = insert_caret(&caret, "§aGreen");
        assert_eq!(shown, format!("{}§aGreen", json_ui::CARET_GLYPH));
    }
}
