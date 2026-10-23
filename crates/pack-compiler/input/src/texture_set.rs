//! Bedrock `*.texture_set.json` schema, as the Bedrock client validates it.
//!
//! Parsing is pure: image references stay unresolved names relative to the definition's own
//! directory in its own pack. Any error discards the whole set, and callers fall back to the
//! plain image of the same name.

use serde_json::{Map, Value};

/// The definition's schema version; each accepts a fixed set of layer keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureSetFormat {
    V1_16_100,
    V1_21_30,
}

/// One layer: an image name or a uniform RGBA value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextureSetLayer {
    Image(Box<str>),
    Uniform([u8; 4]),
}

/// A validated texture set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureSetDefinition {
    pub format: TextureSetFormat,
    pub color: TextureSetLayer,
    /// Normal map image; exclusive with `heightmap`.
    pub normal: Option<Box<str>>,
    /// Single-channel height image; exclusive with `normal`.
    pub heightmap: Option<Box<str>>,
    /// Metalness, emissive and roughness, plus subsurface when `material_has_subsurface`.
    pub material: Option<TextureSetLayer>,
    pub material_has_subsurface: bool,
}

/// Why a definition was discarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureSetError {
    NotAnObject,
    UnsupportedVersion,
    UnknownKey,
    MissingColor,
    InvalidLayer,
    ConflictingLayers,
}

const KEYS_1_16_100: &[&str] = &[
    "color",
    "normal",
    "heightmap",
    "metalness_emissive_roughness",
];
const KEYS_1_21_30: &[&str] = &[
    "color",
    "normal",
    "heightmap",
    "metalness_emissive_roughness",
    "metalness_emissive_roughness_subsurface",
];

/// Validates a parsed definition document.
pub fn parse_texture_set(root: &Value) -> Result<TextureSetDefinition, TextureSetError> {
    let root = root.as_object().ok_or(TextureSetError::NotAnObject)?;
    if root
        .keys()
        .any(|key| !matches!(key.as_str(), "format_version" | "minecraft:texture_set"))
    {
        return Err(TextureSetError::UnknownKey);
    }
    let (format, keys) = match root.get("format_version").and_then(Value::as_str) {
        Some("1.16.100") => (TextureSetFormat::V1_16_100, KEYS_1_16_100),
        Some("1.21.30") => (TextureSetFormat::V1_21_30, KEYS_1_21_30),
        _ => return Err(TextureSetError::UnsupportedVersion),
    };
    let set = root
        .get("minecraft:texture_set")
        .and_then(Value::as_object)
        .ok_or(TextureSetError::NotAnObject)?;
    if set.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(TextureSetError::UnknownKey);
    }
    let field = |key| layer_value(set, key);
    let color = uniform_or_image(field("color")?.ok_or(TextureSetError::MissingColor)?, 4)?;
    let normal = field("normal")?.map(image).transpose()?;
    let heightmap = field("heightmap")?.map(image).transpose()?;
    if normal.is_some() && heightmap.is_some() {
        return Err(TextureSetError::ConflictingLayers);
    }
    let mer = field("metalness_emissive_roughness")?
        .map(|value| uniform_or_image(value, 3))
        .transpose()?;
    let mers = field("metalness_emissive_roughness_subsurface")?
        .map(|value| uniform_or_image(value, 4))
        .transpose()?;
    if mer.is_some() && mers.is_some() {
        return Err(TextureSetError::ConflictingLayers);
    }
    Ok(TextureSetDefinition {
        format,
        color,
        normal,
        heightmap,
        material_has_subsurface: mers.is_some(),
        material: mer.or(mers),
    })
}

/// A present layer value; an explicit `null` invalidates the set.
fn layer_value<'a>(
    set: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Value>, TextureSetError> {
    match set.get(key) {
        None => Ok(None),
        Some(Value::Null) => Err(TextureSetError::InvalidLayer),
        Some(value) => Ok(Some(value)),
    }
}

/// An image name; uniform forms are not accepted for this layer.
fn image(value: &Value) -> Result<Box<str>, TextureSetError> {
    value
        .as_str()
        .filter(|name| !name.is_empty() && !name.starts_with('#'))
        .map(Into::into)
        .ok_or(TextureSetError::InvalidLayer)
}

/// A uniform of exactly `channels` integers, a `#AARRGGBB` hex string, or an image name.
/// Three-channel uniforms are opaque.
fn uniform_or_image(value: &Value, channels: usize) -> Result<TextureSetLayer, TextureSetError> {
    match value {
        Value::Array(values) if values.len() == channels => {
            let mut rgba = [255; 4];
            for (output, value) in rgba.iter_mut().zip(values) {
                // Non-integral numbers and other values log and take full intensity.
                *output = value
                    .as_i64()
                    .map_or(255, |value| value.clamp(0, 255) as u8);
            }
            Ok(TextureSetLayer::Uniform(rgba))
        }
        Value::String(text) if text.starts_with('#') => {
            let [a, r, g, b] = hex_prefix(&text[1..]).to_be_bytes();
            let a = if channels == 3 { 255 } else { a };
            Ok(TextureSetLayer::Uniform([r, g, b, a]))
        }
        Value::String(_) => image(value).map(TextureSetLayer::Image),
        _ => Err(TextureSetError::InvalidLayer),
    }
}

/// Reads leading hexadecimal digits as `strtoul` does: no digits are 0 and overflow saturates.
fn hex_prefix(text: &str) -> u32 {
    let mut value = 0u32;
    for digit in text.chars().map_while(|digit| digit.to_digit(16)) {
        let Some(next) = value
            .checked_mul(16)
            .and_then(|value| value.checked_add(digit))
        else {
            return u32::MAX;
        };
        value = next;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(set: Value) -> Result<TextureSetDefinition, TextureSetError> {
        parse_texture_set(&json!({"format_version": "1.21.30", "minecraft:texture_set": set}))
    }

    #[test]
    fn uniform_arrays_need_exact_counts_and_integral_channels() {
        let set =
            parse(json!({"color": [1, 2.5, -4, 300], "metalness_emissive_roughness": [5, 6, 7]}))
                .unwrap();
        assert_eq!(set.color, TextureSetLayer::Uniform([1, 255, 0, 255]));
        assert_eq!(set.material, Some(TextureSetLayer::Uniform([5, 6, 7, 255])));
        assert!(!set.material_has_subsurface);
        assert_eq!(
            parse(json!({"color": [1, 2, 3]})),
            Err(TextureSetError::InvalidLayer)
        );
    }

    #[test]
    fn hex_uniforms_are_argb_and_mer_stays_opaque() {
        let set = parse(json!({"color": "#80102030", "metalness_emissive_roughness": "#00405060"}))
            .unwrap();
        assert_eq!(
            set.color,
            TextureSetLayer::Uniform([0x10, 0x20, 0x30, 0x80])
        );
        assert_eq!(
            set.material,
            Some(TextureSetLayer::Uniform([0x40, 0x50, 0x60, 255]))
        );
        assert_eq!(
            parse(json!({"color": "#102030"})).unwrap().color,
            TextureSetLayer::Uniform([0x10, 0x20, 0x30, 0]),
            "six digits leave alpha zero"
        );
    }

    #[test]
    fn schema_violations_discard_the_whole_set() {
        for (format, set) in [
            (
                "1.16.100",
                json!({"color": "c", "metalness_emissive_roughness_subsurface": "m"}),
            ),
            (
                "1.21.30",
                json!({"color": "c", "normal": "n", "heightmap": "h"}),
            ),
            (
                "1.21.30",
                json!({"color": "c", "metalness_emissive_roughness": "m", "metalness_emissive_roughness_subsurface": "s"}),
            ),
            (
                "1.21.30",
                json!({"color": "c", "metalness": 0, "emissive": 0, "roughness": 255}),
            ),
            ("1.21.30", json!({"color": "c", "normal": null})),
            ("1.21.30", json!({"color": "c", "normal": "#ffffffff"})),
            ("1.21.30", json!({"normal": "n"})),
            ("1.20.0", json!({"color": "c"})),
        ] {
            assert!(
                parse_texture_set(&json!({"format_version": format, "minecraft:texture_set": set}))
                    .is_err(),
                "{format} {set}"
            );
        }
        assert!(parse_texture_set(&json!({"minecraft:texture_set": {"color": "c"}})).is_err());
    }
}
