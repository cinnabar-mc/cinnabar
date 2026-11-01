//! `pack.mcmeta` and the texture folder layout of a Java Edition pack.

use serde_json::Value;

/// Folder holding the vanilla namespace of a Java pack, relative to its root.
pub(crate) const ASSETS: &str = "assets/minecraft/";
/// Folder holding every texture of a Java pack, relative to its root.
pub(crate) const TEXTURES: &str = "assets/minecraft/textures/";

/// The pack format of 1.13, which renamed `blocks/` and `items/` and flattened texture names.
const FIRST_FLATTENED_FORMAT: i64 = 4;

/// How a Java pack names its block and item texture folders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    /// 1.8-1.12: `textures/blocks`, `textures/items` and pre-flattening names.
    Legacy,
    /// 1.13 and later: `textures/block`, `textures/item` and flattened names.
    Flattened,
}

impl Layout {
    /// Block and item folders, relative to [`TEXTURES`], each ending in `/`.
    pub(crate) fn folders(self) -> (&'static str, &'static str) {
        match self {
            Self::Legacy => ("blocks/", "items/"),
            Self::Flattened => ("block/", "item/"),
        }
    }
}

/// What the translation reads from `pack.mcmeta`.
#[derive(Debug, Default)]
pub(crate) struct PackMeta {
    /// `pack_format`, or the major `min_format` of 1.21.9+ packs; 0 when absent or unreadable.
    pub(crate) format: i64,
    /// The description as plain text without formatting codes.
    pub(crate) description: String,
}

/// Reads `pack.mcmeta`, leaving defaults for anything missing or malformed.
pub(crate) fn read_meta(bytes: Option<&[u8]>) -> PackMeta {
    let bytes = bytes.map(|bytes| bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes));
    let Some(root) = bytes.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok()) else {
        return PackMeta::default();
    };
    let pack = &root["pack"];
    let format = pack["pack_format"]
        .as_i64()
        .filter(|format| *format != 0)
        .or_else(|| major_format(&pack["min_format"]))
        .unwrap_or_default();
    PackMeta {
        format,
        description: text_component(&pack["description"]),
    }
}

/// Decodes a 1.21.9+ format, written as a number or as `[major, minor]`.
fn major_format(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_array()?.first()?.as_i64())
}

/// Flattens a Java text component (string, object or array) into plain text without `§` codes.
pub(crate) fn text_component(value: &Value) -> String {
    /// Appends the text of `value` and its children in reading order.
    fn walk(value: &Value, out: &mut String, depth: usize) {
        if depth > 64 {
            return;
        }
        match value {
            Value::String(text) => out.push_str(text),
            Value::Array(values) => values.iter().for_each(|value| walk(value, out, depth + 1)),
            Value::Object(object) => {
                if let Some(text) = object.get("text") {
                    walk(text, out, depth + 1);
                }
                if let Some(extra) = object.get("extra") {
                    walk(extra, out, depth + 1);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    walk(value, &mut text, 0);
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '§' {
            characters.next();
        } else {
            plain.push(character);
        }
    }
    plain.trim().to_owned()
}

/// Picks the layout from the folders present, falling back to the pack format.
pub(crate) fn detect_layout<'a>(paths: impl IntoIterator<Item = &'a str>, format: i64) -> Layout {
    let mut legacy = false;
    for path in paths {
        let Some(relative) = path.strip_prefix(TEXTURES) else {
            continue;
        };
        if relative.starts_with("block/") || relative.starts_with("item/") {
            return Layout::Flattened;
        }
        legacy |= relative.starts_with("blocks/") || relative.starts_with("items/");
    }
    if legacy || format < FIRST_FLATTENED_FORMAT {
        Layout::Legacy
    } else {
        Layout::Flattened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_flatten_components_and_strip_formatting() {
        let meta = read_meta(Some(
            "{\"pack\":{\"pack_format\":1,\"description\":\"\u{a7}6Legacy \u{a7}rPvP\"}}"
                .as_bytes(),
        ));
        assert_eq!((meta.format, meta.description.as_str()), (1, "Legacy PvP"));
        let meta = read_meta(Some(
            br#"{"pack":{"description":{"text":"Current ","extra":[{"text":"Pack"}]},"min_format":[97,1]}}"#,
        ));
        assert_eq!(
            (meta.format, meta.description.as_str()),
            (97, "Current Pack")
        );
        assert_eq!(read_meta(Some(b"not json")).format, 0);
    }

    #[test]
    fn folders_decide_the_layout_before_the_format() {
        let flattened = ["assets/minecraft/textures/block/stone.png"];
        let legacy = ["assets/minecraft/textures/items/apple.png"];
        assert_eq!(detect_layout(flattened, 1), Layout::Flattened);
        assert_eq!(detect_layout(legacy, 48), Layout::Legacy);
        assert_eq!(detect_layout([], 48), Layout::Flattened);
        assert_eq!(detect_layout([], 3), Layout::Legacy);
    }
}
