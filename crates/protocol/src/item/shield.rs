//! The banner decoration retained in a shield's item data.

use super::{decode_extra_nbt, read_i32_le, read_u8, read_u16_le, root_tag, skip_le_payload};
use render_api::{BannerPattern, SHIELD_PATTERN_LAYERS, ShieldPattern};
use std::sync::Arc;

/// Reads shield decoration without treating its blocking timestamp as NBT.
#[must_use]
pub fn item_shield_pattern(extra_data: &[u8]) -> Option<ShieldPattern> {
    let nbt = decode_extra_nbt(extra_data)?;
    let base = root_tag(&mut &nbt[..], 3, b"Base").and_then(|mut value| read_i32_le(&mut value));
    let mut layers = Vec::new();
    if let Some(mut list) = root_tag(&mut &nbt[..], 9, b"Patterns")
        && read_u8(&mut list)? == 10
    {
        let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
        for _ in 0..count.min(SHIELD_PATTERN_LAYERS) {
            let (mut pattern, mut color) = (Arc::from(""), 0);
            loop {
                let tag = read_u8(&mut list)?;
                if tag == 0 {
                    break;
                }
                let length = usize::from(read_u16_le(&mut list)?);
                let name = list.get(..length)?;
                list = list.get(length..)?;
                match (tag, name) {
                    (8, b"Pattern") => {
                        let length = usize::from(read_u16_le(&mut list)?);
                        pattern = Arc::from(std::str::from_utf8(list.get(..length)?).ok()?);
                        list = list.get(length..)?;
                    }
                    (3, b"Color") => color = read_i32_le(&mut list)? as u8,
                    _ => skip_le_payload(&mut list, tag, 1)?,
                }
            }
            layers.push(BannerPattern { pattern, color });
        }
    }
    ShieldPattern::new(base, layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes original fixed little-endian item metadata with a separate shield timestamp.
    fn fixture(base: Option<i32>, layers: &[(&str, Option<i32>)], tick: i64) -> Vec<u8> {
        let mut bytes = vec![255, 255, 1, 10, 0, 0];
        if let Some(base) = base {
            bytes.extend([3, 4, 0]);
            bytes.extend(b"Base");
            bytes.extend(base.to_le_bytes());
        }
        bytes.extend([9, 8, 0]);
        bytes.extend(b"Patterns");
        bytes.push(10);
        bytes.extend((layers.len() as i32).to_le_bytes());
        for (pattern, color) in layers {
            bytes.extend([8, 7, 0]);
            bytes.extend(b"Pattern");
            bytes.extend((pattern.len() as u16).to_le_bytes());
            bytes.extend(pattern.as_bytes());
            if let Some(color) = color {
                bytes.extend([3, 5, 0]);
                bytes.extend(b"Color");
                bytes.extend(color.to_le_bytes());
            }
            bytes.push(0);
        }
        bytes.push(0);
        bytes.extend([0; 8]);
        bytes.extend(tick.to_le_bytes());
        bytes
    }

    #[test]
    fn shield_decoration_is_independent_of_blocking_timestamp() {
        let first = item_shield_pattern(&fixture(Some(15), &[("sc", Some(1)), ("bo", Some(4))], 0))
            .unwrap();
        let later = item_shield_pattern(&fixture(
            Some(15),
            &[("sc", Some(1)), ("bo", Some(4))],
            12345,
        ))
        .unwrap();
        assert_eq!(first, later);
        assert_eq!(first.base, 15);
        assert_eq!(
            first
                .layers
                .iter()
                .map(|layer| (&*layer.pattern, layer.color))
                .collect::<Vec<_>>(),
            [("sc", 1), ("bo", 4)]
        );
        assert_eq!(item_shield_pattern(&fixture(None, &[], 0)), None);
        assert_eq!(item_shield_pattern(&fixture(Some(-1), &[], 0)), None);
    }

    #[test]
    fn shield_decoration_retains_the_first_six_layers_and_native_color_defaults() {
        let layers = [
            ("unrecognized", None),
            ("sc", Some(-1)),
            ("bo", Some(260)),
            ("bs", Some(0)),
            ("ts", Some(1)),
            ("cs", Some(2)),
            ("ms", Some(3)),
        ];
        let first = item_shield_pattern(&fixture(Some(-1), &layers, 0)).unwrap();
        assert_eq!(first.base, 15);
        assert_eq!(first.layers.len(), SHIELD_PATTERN_LAYERS);
        assert_eq!(first.layers[0].pattern.as_ref(), "unrecognized");
        assert_eq!(
            first
                .layers
                .iter()
                .map(|layer| layer.color)
                .collect::<Vec<_>>(),
            [0, 255, 4, 0, 1, 2]
        );
        let absent = item_shield_pattern(&fixture(None, &layers[..1], 0)).unwrap();
        assert_eq!(absent.base, 0);
    }
}
