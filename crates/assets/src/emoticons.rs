//! Text emoticons: received text names a private-use glyph as `:name:`, such as
//! `:xbox_face_button_down:`, and the client swaps the name for the glyph's code point.
//!
//! Vanilla reads its name list from the game's own `font/emoticons` file and draws the code
//! points from its built-in private-use font pages. Neither ships in the fetched sample pack, so
//! Cinnabar carries its own list of the controller and mouse emoticons that control hints name,
//! drawn with original art on the private-use page that holds them.

use std::{collections::HashMap, sync::LazyLock};

use crate::{CellGlyph, GlyphSheet, SHEET_GRID, extract_cells};

mod art;

/// The private-use page every built-in emoticon is drawn on.
pub const BUILTIN_EMOTICON_PAGE: u8 = 0xe0;

/// One built-in emoticon: its name, code point and the original art that draws it.
struct BuiltinEmoticon {
    name: &'static str,
    codepoint: char,
    art: art::Art,
}

/// Maps emoticon names, without their colons, to the code points that replace them.
#[derive(Debug, Default)]
pub struct EmoticonList {
    codepoints: HashMap<&'static str, char>,
}

impl EmoticonList {
    /// The emoticons Cinnabar draws itself.
    pub fn builtin() -> &'static Self {
        static LIST: LazyLock<EmoticonList> = LazyLock::new(|| EmoticonList {
            codepoints: art::BUILTIN
                .iter()
                .map(|emoticon| (emoticon.name, emoticon.codepoint))
                .collect(),
        });
        &LIST
    }

    /// The code point that `:name:` turns into, if the list names it.
    pub fn get(&self, name: &str) -> Option<char> {
        self.codepoints.get(name).copied()
    }

    /// Returns `text` with every listed `:name:` replaced by its code point, or `None` when
    /// nothing was replaced.
    ///
    /// Follows vanilla's scan: a `:` opens a name that runs over ASCII letters, digits and
    /// underscores; any other character abandons it. The next `:` closes the name, and when the
    /// name is unlisted that `:` opens the next one. Scanning resumes after an inserted glyph.
    /// Every listed code point is in the Basic Multilingual Plane, so the result is never
    /// longer than `text`.
    pub fn emoticonify(&self, text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let mut result = String::new();
        let mut copied = 0;
        let mut open = None;
        for (index, &byte) in bytes.iter().enumerate() {
            if byte == b':' {
                // Name bytes are ASCII, so the span between the colons is on char boundaries.
                let listed =
                    open.and_then(|start: usize| Some((start, self.get(&text[start + 1..index])?)));
                if let Some((start, codepoint)) = listed {
                    result.push_str(&text[copied..start]);
                    result.push(codepoint);
                    copied = index + 1;
                    open = None;
                } else {
                    open = Some(index);
                }
            } else if !(byte.is_ascii_alphanumeric() || byte == b'_') {
                open = None;
            }
        }
        if copied == 0 {
            return None;
        }
        result.push_str(&text[copied..]);
        Some(result)
    }
}

/// The built-in emoticon page as a glyph sheet of 16-pixel cells.
pub fn builtin_emoticon_sheet() -> GlyphSheet {
    const CELL: u32 = 16;
    let side = CELL * SHEET_GRID;
    let mut rgba8 = vec![0; (side * side * 4) as usize];
    for emoticon in &art::BUILTIN {
        let index = emoticon.codepoint as u32 & 0xff;
        let rows = emoticon.art.rows();
        let height = rows.len() as u32;
        let width = rows.iter().map(|row| row.len()).max().unwrap_or(0) as u32;
        let origin = [
            index % SHEET_GRID * CELL + (CELL - width) / 2,
            index / SHEET_GRID * CELL + (CELL - height) / 2,
        ];
        for (y, row) in (0..).zip(&rows) {
            for (x, symbol) in (0..).zip(row.bytes()) {
                let Some(color) = art::color(symbol) else {
                    continue;
                };
                let texel = (((origin[1] + y) * side + origin[0] + x) * 4) as usize;
                rgba8[texel..texel + 4].copy_from_slice(&color);
            }
        }
    }
    GlyphSheet {
        high_byte: BUILTIN_EMOTICON_PAGE,
        width: side,
        height: side,
        rgba8: rgba8.into(),
    }
}

/// The drawn cells of the built-in emoticon page; its blank cells are left out so they do not
/// hide any other font's glyphs.
pub fn builtin_emoticon_cells() -> Vec<CellGlyph> {
    extract_cells(&builtin_emoticon_sheet())
        .into_iter()
        .filter(|cell| cell.size != [0, 0])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names_and_code_points_are_unique_and_drawn() {
        let cells = builtin_emoticon_cells();
        assert_eq!(cells.len(), art::BUILTIN.len());
        let mut codepoints = std::collections::HashSet::new();
        for emoticon in &art::BUILTIN {
            assert_eq!(
                EmoticonList::builtin().get(emoticon.name),
                Some(emoticon.codepoint),
                "{} is listed once",
                emoticon.name
            );
            assert!(codepoints.insert(emoticon.codepoint));
            assert_eq!(
                emoticon.codepoint as u32 >> 8,
                u32::from(BUILTIN_EMOTICON_PAGE)
            );
            let rows = emoticon.art.rows();
            assert!(rows.len() <= 16 && rows.iter().all(|row| row.len() <= 16));
            assert!(
                rows.iter()
                    .flat_map(|row| row.bytes())
                    .all(|symbol| symbol == b'.' || art::color(symbol).is_some()),
                "{} uses only palette colors",
                emoticon.name
            );
            assert!(
                cells
                    .iter()
                    .any(|cell| cell.codepoint == emoticon.codepoint),
                "{} has art",
                emoticon.name
            );
        }
    }

    #[test]
    fn replaces_listed_names_with_their_code_points() {
        let list = EmoticonList::builtin();
        let a = list.get("xbox_face_button_down").unwrap();
        let mouse = list.get("mouse_left_button").unwrap();
        assert_eq!(
            list.emoticonify("Press :xbox_face_button_down: or :mouse_left_button:!"),
            Some(format!("Press {a} or {mouse}!"))
        );
        assert_eq!(list.emoticonify("no names here"), None);
        assert_eq!(list.emoticonify(":unknown: :heart"), None);
        assert_eq!(
            list.emoticonify("é::xbox_face_button_down::"),
            Some(format!("é:{a}:"))
        );
    }

    /// An unlisted name's closing colon opens the next name; other characters abandon it.
    #[test]
    fn follows_the_vanilla_scan() {
        let list = EmoticonList::builtin();
        let a = list.get("xbox_face_button_down").unwrap();
        assert_eq!(
            list.emoticonify("10:30:xbox_face_button_down:"),
            Some(format!("10:30{a}"))
        );
        assert_eq!(list.emoticonify(":xbox face_button_down:"), None);
        assert_eq!(list.emoticonify(":xbox-face_button_down:"), None);
        assert_eq!(
            list.emoticonify(":xbox_face_button_down::xbox_face_button_down:"),
            Some(format!("{a}{a}"))
        );
    }
}
