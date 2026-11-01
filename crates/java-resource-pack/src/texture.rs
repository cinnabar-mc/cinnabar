//! PNG decoding, encoding and the pixel operations the translation needs.

use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use std::io::Cursor;

/// Largest width or height decoded for one texture; HD Java atlases stay well below it.
const MAX_DIMENSION: u32 = 16_384;
/// Largest allocation one decode may make, so a hostile header cannot exhaust memory.
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

/// Opens PNG bytes with the translation's decode limits.
fn reader(bytes: &[u8]) -> ImageReader<Cursor<&[u8]>> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    reader
}

/// Decodes PNG bytes into 8-bit RGBA, or `None` when they are not a PNG within the limits.
pub(crate) fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    reader(bytes).decode().ok().map(|image| image.into_rgba8())
}

/// Reads a PNG's width and height from its header without decoding pixels.
pub(crate) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    reader(bytes).into_dimensions().ok()
}

/// Encodes an image as an RGBA PNG.
pub(crate) fn encode(image: &RgbaImage) -> Vec<u8> {
    use image::{
        ExtendedColorType, ImageEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .expect("encoding an in-memory RGBA image cannot fail");
    bytes
}

/// Makes every partially transparent pixel fully transparent, because Bedrock draws items and HUD
/// icons with alpha testing. Returns whether any pixel changed.
pub(crate) fn fix_alpha(image: &mut RgbaImage) -> bool {
    let mut changed = false;
    for pixel in image.pixels_mut() {
        if pixel.0[3] != 0 && pixel.0[3] != 255 {
            pixel.0 = [0; 4];
            changed = true;
        }
    }
    changed
}

/// Whether every pixel is fully transparent.
pub(crate) fn is_empty(image: &RgbaImage) -> bool {
    image.pixels().all(|pixel| pixel.0[3] == 0)
}

/// Copies the `width` x `height` region at (`x`, `y`); parts outside the image stay transparent.
pub(crate) fn crop(image: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
    let mut out = RgbaImage::new(width, height);
    for out_y in 0..height {
        let Some(source_y) = y.checked_add(out_y).filter(|row| *row < image.height()) else {
            break;
        };
        for out_x in 0..width {
            let Some(source_x) = x
                .checked_add(out_x)
                .filter(|column| *column < image.width())
            else {
                break;
            };
            out.put_pixel(out_x, out_y, *image.get_pixel(source_x, source_y));
        }
    }
    out
}

/// The side of the top square frame when the image is a vertical animation strip.
pub(crate) fn strip_frame(width: u32, height: u32) -> Option<u32> {
    (width != 0 && height >= 2 * width && height.is_multiple_of(width)).then_some(width)
}

/// Multiplies every pixel's colour by `color`, the way Minecraft tints item layers.
pub(crate) fn tint(image: &RgbaImage, color: [u8; 3]) -> RgbaImage {
    let mut out = image.clone();
    for pixel in out.pixels_mut() {
        for (channel, factor) in pixel.0.iter_mut().zip(color) {
            *channel = (u16::from(*channel) * u16::from(factor) / 255) as u8;
        }
    }
    out
}

/// Composites `top` over `base`, both aligned at their top-left corners, with source-over blending
/// on 16-bit premultiplied values.
pub(crate) fn over(base: &RgbaImage, top: &RgbaImage) -> RgbaImage {
    const MAX: u32 = 0xffff;
    let premultiplied = |pixel: [u8; 4]| {
        let alpha = u32::from(pixel[3]) * 0x101;
        let channel = |value: u8| u32::from(value) * 0x101 * alpha / MAX;
        [
            channel(pixel[0]),
            channel(pixel[1]),
            channel(pixel[2]),
            alpha,
        ]
    };
    let mut out = base.clone();
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        if x >= top.width() || y >= top.height() {
            continue;
        }
        let source = premultiplied(top.get_pixel(x, y).0);
        let destination = premultiplied(pixel.0);
        let keep = MAX - source[3];
        let blended: [u32; 4] =
            std::array::from_fn(|index| destination[index] * keep / MAX + source[index]);
        let alpha = blended[3];
        pixel.0 = if alpha == 0 {
            [0; 4]
        } else {
            let channel = |value: u32| ((value * MAX / alpha) >> 8) as u8;
            [
                channel(blended[0]),
                channel(blended[1]),
                channel(blended[2]),
                (alpha >> 8) as u8,
            ]
        };
    }
    out
}

/// Copies `source` onto `canvas` with its top-left corner at (`x`, `y`), replacing pixels.
pub(crate) fn paste(canvas: &mut RgbaImage, source: &RgbaImage, x: u32, y: u32) {
    image::imageops::replace(canvas, source, i64::from(x), i64::from(y));
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn crop_keeps_parts_outside_the_source_transparent() {
        let image = RgbaImage::from_pixel(2, 2, Rgba([9, 9, 9, 255]));
        let cropped = crop(&image, 1, 1, 3, 3);
        assert_eq!(cropped.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(cropped.get_pixel(1, 0).0, [0; 4]);
        assert_eq!(cropped.get_pixel(0, 2).0, [0; 4]);
    }

    #[test]
    fn alpha_fix_clears_only_partial_alpha() {
        let mut image = RgbaImage::from_fn(3, 1, |x, _| Rgba([1, 2, 3, [0, 128, 255][x as usize]]));
        assert!(fix_alpha(&mut image));
        assert_eq!(image.get_pixel(1, 0).0, [0; 4]);
        assert_eq!(image.get_pixel(2, 0).0, [1, 2, 3, 255]);
        assert!(!fix_alpha(&mut image));
    }

    #[test]
    fn over_keeps_the_base_under_transparent_pixels_and_replaces_it_under_opaque_ones() {
        let base = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
        let top = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([0; 4])
            } else {
                Rgba([200, 100, 50, 255])
            }
        });
        let out = over(&base, &top);
        assert_eq!(out.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [200, 100, 50, 255]);
    }

    #[test]
    fn strips_are_square_multiples_at_least_two_frames_tall() {
        assert_eq!(strip_frame(16, 64), Some(16));
        assert_eq!(strip_frame(16, 16), None);
        assert_eq!(strip_frame(16, 40), None);
    }
}
