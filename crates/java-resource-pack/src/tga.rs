//! Textures vanilla ships as TGA. The client tries `.tga` before `.png` across every pack layer, so
//! translated art for these paths is written as TGA, and biome-tinted grass sides are composed
//! into Bedrock's tint-mask form.

use crate::{
    meta::Layout,
    texture::{Budget, canvas},
};
use image::RgbaImage;
use std::{collections::HashMap, sync::LazyLock};

const TGA_TEXTURES: &str = include_str!("../data/tga-textures.tsv");

/// Java sources of a tint mask: a base texture and an optional overlay, as `folder/name` keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaskSources {
    pub(crate) base: &'static str,
    pub(crate) overlay: Option<&'static str>,
}

/// How a vanilla TGA texture is translated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TgaKind {
    /// Alpha is plain 0/255 transparency: the Java art is written as TGA.
    Cutout,
    /// Alpha 255 marks biome-tinted pixels and 0 untinted ones, composed from Java sources.
    TintMask {
        legacy: MaskSources,
        flattened: MaskSources,
    },
    /// Alpha carries a dye encoding with no Java equivalent: not translated.
    DyeMask,
}

/// Parses one `base<TAB>overlay` pair, where `-` means no overlay.
fn mask_sources(base: &'static str, overlay: &'static str) -> MaskSources {
    MaskSources {
        base,
        overlay: (overlay != "-").then_some(overlay),
    }
}

/// Parses the bundled table, panicking on a malformed line so a bad edit fails every test.
fn parse(text: &'static str) -> HashMap<&'static str, TgaKind> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let kind = match fields.as_slice() {
                [_, "cutout"] => TgaKind::Cutout,
                [_, "dye-mask"] => TgaKind::DyeMask,
                [
                    _,
                    "tint-mask",
                    legacy_base,
                    legacy_overlay,
                    flat_base,
                    flat_overlay,
                ] => TgaKind::TintMask {
                    legacy: mask_sources(legacy_base, legacy_overlay),
                    flattened: mask_sources(flat_base, flat_overlay),
                },
                _ => panic!("malformed TGA texture line: {line}"),
            };
            (fields[0], kind)
        })
        .collect()
}

static KINDS: LazyLock<HashMap<&str, TgaKind>> = LazyLock::new(|| parse(TGA_TEXTURES));

/// How the vanilla texture at `path` (relative to `textures/`, without extension) is translated
/// when vanilla ships it as TGA.
pub(crate) fn kind(path: &str) -> Option<TgaKind> {
    KINDS.get(path).copied()
}

/// Every tint mask with the Java sources for `layout`, as `(Bedrock path, sources)`.
pub(crate) fn tint_masks(layout: Layout) -> impl Iterator<Item = (&'static str, MaskSources)> {
    KINDS.iter().filter_map(move |(path, kind)| match kind {
        TgaKind::TintMask { legacy, flattened } => Some((
            *path,
            match layout {
                Layout::Legacy => *legacy,
                Layout::Flattened => *flattened,
            },
        )),
        _ => None,
    })
}

/// Composes a tint mask: where the overlay is at least half opaque its colour is kept with alpha
/// 255 (tinted), elsewhere the base colour is kept with alpha 0 (untinted). The overlay must match
/// the base's size.
pub(crate) fn tint_mask(
    base: &RgbaImage,
    overlay: Option<&RgbaImage>,
    budget: &Budget,
) -> Option<RgbaImage> {
    if overlay.is_some_and(|overlay| overlay.dimensions() != base.dimensions()) {
        return None;
    }
    let mut out = canvas(base.width(), base.height(), budget)?;
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let [r, g, b, _] = base.get_pixel(x, y).0;
        pixel.0 = match overlay.map(|overlay| overlay.get_pixel(x, y).0) {
            Some([r, g, b, a]) if a >= 128 => [r, g, b, 255],
            _ => [r, g, b, 0],
        };
    }
    Some(out)
}

/// Encodes an image as an uncompressed 32-bit TGA with a bottom-left origin, the layout of most
/// vanilla TGA textures, which every TGA decoder reads.
pub(crate) fn encode(image: &RgbaImage) -> Vec<u8> {
    let (width, height) = image.dimensions();
    let mut bytes = Vec::with_capacity(18 + image.as_raw().len());
    bytes.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // TGA stores dimensions in 16 bits; decoded textures stay within the decoder's limits.
    bytes.extend_from_slice(&(width as u16).to_le_bytes());
    bytes.extend_from_slice(&(height as u16).to_le_bytes());
    bytes.extend_from_slice(&[32, 8]);
    for y in (0..height).rev() {
        for x in 0..width {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            bytes.extend_from_slice(&[b, g, r, a]);
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn the_table_parses_with_both_grass_masks() {
        assert_eq!(parse(TGA_TEXTURES).len(), KINDS.len());
        assert_eq!(kind("blocks/leaves_oak"), Some(TgaKind::Cutout));
        assert_eq!(kind("blocks/stone"), None);
        assert_eq!(tint_masks(Layout::Legacy).count(), 2);
    }

    #[test]
    fn the_table_matches_the_pinned_sample_pack() {
        let Some(pack) = crate::test_support::sample_pack() else {
            return;
        };
        let mut vanilla = Vec::new();
        for folder in ["blocks", "items", "models/armor"] {
            for entry in std::fs::read_dir(pack.join("textures").join(folder)).unwrap() {
                let name = entry.unwrap().file_name().into_string().unwrap();
                let Some(stem) = name.strip_suffix(".tga") else {
                    continue;
                };
                if !["_mers", "_normal", "_heightmap"]
                    .iter()
                    .any(|suffix| stem.ends_with(suffix))
                {
                    vanilla.push(format!("{folder}/{stem}"));
                }
            }
        }
        vanilla.sort();
        let mut listed: Vec<&str> = KINDS.keys().copied().collect();
        listed.sort_unstable();
        assert_eq!(listed, vanilla);
        for (path, kind) in KINDS.iter() {
            let image = image::open(pack.join("textures").join(format!("{path}.tga")))
                .unwrap()
                .into_rgba8();
            let plain_alpha = image.pixels().all(|pixel| matches!(pixel.0[3], 0 | 255));
            assert_eq!(*kind == TgaKind::DyeMask, !plain_alpha, "{path}");
        }
    }

    #[test]
    fn masks_keep_overlay_pixels_tinted_and_base_pixels_untinted() {
        let base = RgbaImage::from_pixel(2, 1, Rgba([100, 60, 40, 255]));
        let overlay = RgbaImage::from_fn(2, 1, |x, _| Rgba([150, 150, 150, [255, 0][x as usize]]));
        let mask = tint_mask(&base, Some(&overlay), &Budget::new()).unwrap();
        assert_eq!(mask.get_pixel(0, 0).0, [150, 150, 150, 255]);
        assert_eq!(mask.get_pixel(1, 0).0, [100, 60, 40, 0]);
        let untinted = tint_mask(&base, None, &Budget::new()).unwrap();
        assert_eq!(untinted.get_pixel(0, 0).0, [100, 60, 40, 0]);
    }

    #[test]
    fn tga_uses_the_vanilla_header_and_round_trips() {
        let image = RgbaImage::from_fn(5, 3, |x, y| {
            Rgba([x as u8, y as u8, 7, [0, 255][(x % 2) as usize]])
        });
        let bytes = encode(&image);
        assert_eq!((bytes[2], bytes[16], bytes[17]), (2, 32, 8));
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded, image);
    }
}
