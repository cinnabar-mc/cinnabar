//! Shield cloth colors composed from the active pack's original texture sources.

use crate::{EquipmentTexture, banner};
use std::sync::Arc;
mod icon;
pub use icon::bake as bake_icon;

/// Runtime artwork directory shared by every shield overlay.
pub const PATTERN_DIRECTORY: &str = "textures/entity/shield_patterns";

/// Derives shield artwork routes from the shared banner pattern identities.
pub fn pattern_paths() -> impl Iterator<Item = String> {
    banner::BANNER_PATTERNS
        .iter()
        .map(|pattern| pattern_path(pattern.code))
}

/// Resolves a wire pattern code, using the cloth base for an unknown code.
#[must_use]
pub fn pattern_path(code: &str) -> String {
    let texture = banner::pattern_texture(code).unwrap_or("base");
    format!("{PATTERN_DIRECTORY}/{texture}")
}

/// Composes every layer within the output pixel budget admitted by its consumer.
/// Texture sampling uses normalized coordinates, preserving custom pack dimensions.
/// Mask alpha blends colors; the patterned material outputs opaque texels.
pub fn compose<'a, 'p>(
    shield: &'a EquipmentTexture,
    base: u8,
    layers: impl IntoIterator<Item = (&'p str, u8)>,
    max_pixels: usize,
    texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Option<EquipmentTexture> {
    let sources = pattern_sources(base, layers, texture);
    let [mut width, mut height] = source_size(shield, &sources)?;
    if max_pixels == 0 {
        return None;
    }
    while usize::from(width) * usize::from(height) > max_pixels {
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    for y in 0..height {
        for x in 0..width {
            let sample = |texture: &EquipmentTexture| {
                let sx = usize::from(x) * usize::from(texture.width) / usize::from(width);
                let sy = usize::from(y) * usize::from(texture.height) / usize::from(height);
                let at = (sy * usize::from(texture.width) + sx) * 4;
                <[u8; 4]>::try_from(&texture.rgba8[at..at + 4]).expect("validated texture sample")
            };
            let mut result = sample(shield).map(|value| f32::from(value) / 255.0);
            for (texture, color) in &sources {
                let mask = sample(texture);
                let rgb = if *color < 16 {
                    banner::color_rgb(i64::from(*color))
                } else {
                    [255; 3]
                };
                let alpha = f32::from(mask[3]) / 255.0;
                for channel in 0..3 {
                    let next = f32::from(mask[channel]) * f32::from(rgb[channel]) / (255.0 * 255.0);
                    result[channel] += (next - result[channel]) * alpha;
                }
            }
            pixels.extend(
                result[..3]
                    .iter()
                    .map(|value| (value * 255.0).round() as u8),
            );
            pixels.push(255);
        }
    }
    Some(EquipmentTexture {
        identifier: shield.identifier.clone(),
        width,
        height,
        rgba8: Arc::from(pixels),
    })
}

/// Measures the composed source extent before any output pixels are allocated.
pub fn composition_size<'a, 'p>(
    shield: &'a EquipmentTexture,
    base: u8,
    layers: impl IntoIterator<Item = (&'p str, u8)>,
    texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Option<[u16; 2]> {
    source_size(shield, &pattern_sources(base, layers, texture))
}

/// Resolves only available masks in their authored blend order.
fn pattern_sources<'a, 'p>(
    base: u8,
    layers: impl IntoIterator<Item = (&'p str, u8)>,
    mut texture: impl FnMut(&str) -> Option<&'a EquipmentTexture>,
) -> Vec<(&'a EquipmentTexture, u8)> {
    std::iter::once(("", base))
        .chain(layers)
        .filter_map(|(code, color)| texture(&pattern_path(code)).map(|texture| (texture, color)))
        .collect()
}

/// Validates all borrowed rasters and determines their normalized sampling frame.
fn source_size(shield: &EquipmentTexture, sources: &[(&EquipmentTexture, u8)]) -> Option<[u16; 2]> {
    let valid = |texture: &EquipmentTexture| {
        texture.width > 0
            && texture.height > 0
            && texture.rgba8.len() == usize::from(texture.width) * usize::from(texture.height) * 4
    };
    if !valid(shield) || sources.iter().any(|(texture, _)| !valid(texture)) {
        return None;
    }
    let width = sources
        .iter()
        .fold(shield.width, |width, (texture, _)| width.max(texture.width));
    let height = sources.iter().fold(shield.height, |height, (texture, _)| {
        height.max(texture.height)
    });
    Some([width, height])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes a single-row original raster with a recognizable pixel for each test layer.
    fn raster(name: &str, pixels: &[[u8; 4]]) -> EquipmentTexture {
        EquipmentTexture {
            identifier: name.into(),
            width: pixels.len() as u16,
            height: 1,
            rgba8: pixels.iter().flatten().copied().collect::<Vec<_>>().into(),
        }
    }

    #[test]
    fn shield_composition_respects_admitted_pixels_for_crossed_strip_masks() {
        let shield = raster("fixture:shield", &[[10, 20, 30, 255]; 128]);
        let mut tall = raster("base", &[[255; 4]; 128]);
        tall.width = 1;
        tall.height = 128;
        let result = compose(&shield, 15, [], 1024, |_| Some(&tall)).unwrap();
        assert!(usize::from(result.width) * usize::from(result.height) <= 1024);
        assert_eq!(&result.rgba8[..3], banner::color_rgb(15));
        assert!(compose(&shield, 15, [], 0, |_| Some(&tall)).is_none());
    }

    #[test]
    fn patterned_material_outputs_opaque_texels_even_outside_the_cloth_mask() {
        let shield = raster("fixture:shield", &[[10, 20, 30, 0], [40, 50, 60, 128]]);
        let mask = raster("base", &[[0; 4]; 2]);
        let result = compose(&shield, 15, [], usize::MAX, |_| Some(&mask)).unwrap();
        assert_eq!(result.rgba8.as_ref(), &[10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn shield_patterns_keep_layer_order_alpha_and_uncovered_frame_texels() {
        let shield = raster("fixture:shield", &[[10, 20, 30, 255]; 2]);
        let base = raster("base", &[[255, 255, 255, 255], [0; 4]]);
        let cross = raster("straight_cross", &[[255, 255, 255, 128], [0; 4]]);
        let border = raster("border", &[[255; 4], [0; 4]]);
        let resolve = |path: &str| match path.rsplit('/').next()? {
            "base" => Some(&base),
            "straight_cross" => Some(&cross),
            "border" => Some(&border),
            _ => None,
        };
        let partial = compose(&shield, 15, [("sc", 1)], usize::MAX, resolve).unwrap();
        assert_eq!(&partial.rgba8[4..], &[10, 20, 30, 255]);
        let white = banner::color_rgb(15);
        let red = banner::color_rgb(1);
        let expected = std::array::from_fn::<_, 3, _>(|i| {
            ((f32::from(white[i]) * 127.0 + f32::from(red[i]) * 128.0) / 255.0).round() as u8
        });
        assert_eq!(&partial.rgba8[..3], expected);
        let ordered = compose(&shield, 15, [("sc", 1), ("bo", 4)], usize::MAX, resolve).unwrap();
        assert_eq!(&ordered.rgba8[..3], banner::color_rgb(4));
    }
}
