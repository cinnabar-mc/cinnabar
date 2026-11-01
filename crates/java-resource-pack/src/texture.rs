//! PNG decoding, encoding and the pixel operations the translation needs.

use image::{ImageFormat, ImageReader, Limits, RgbaImage};
use std::{
    io::Cursor,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

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

/// Pixels one translation may hold in decoded or derived images: 1 GiB of RGBA, enough for a
/// 512x pack's atlases, HUD and composed items, while bounding what a small archive can demand.
const MAX_TRANSLATION_PIXELS: u64 = 1 << 28;

/// The cumulative pixel allowance of one translation. Every decode and derived image reserves
/// its pixels before allocating, so the total held stays bounded however work is parallelized.
pub(crate) struct Budget {
    remaining: AtomicU64,
    exhausted: AtomicBool,
}

impl Budget {
    /// A full allowance of [`MAX_TRANSLATION_PIXELS`].
    pub(crate) fn new() -> Self {
        Self::with_pixels(MAX_TRANSLATION_PIXELS)
    }

    /// An allowance of `pixels`.
    pub(crate) fn with_pixels(pixels: u64) -> Self {
        Self {
            remaining: AtomicU64::new(pixels),
            exhausted: AtomicBool::new(false),
        }
    }

    /// Reserves a `width` x `height` image. An image over the per-image limit is refused on its
    /// own; running out of the translation's allowance is also recorded as exhaustion.
    pub(crate) fn reserve(&self, width: u32, height: u32) -> bool {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_DERIVED_PIXELS {
            return false;
        }
        let reserved = self
            .remaining
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(pixels)
            })
            .is_ok();
        if !reserved {
            self.exhausted.store(true, Ordering::Relaxed);
        }
        reserved
    }

    /// Whether any reservation was refused.
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted.load(Ordering::Relaxed)
    }
}

/// Decodes PNG bytes into 8-bit RGBA after reserving their pixels, or `None` when they are not a
/// PNG within the limits or the budget is spent.
pub(crate) fn decode(bytes: &[u8], budget: &Budget) -> Option<RgbaImage> {
    let (width, height) = dimensions(bytes)?;
    if !budget.reserve(width, height) {
        return None;
    }
    reader(bytes).decode().ok().map(|image| image.into_rgba8())
}

/// Copies an image after reserving its pixels.
pub(crate) fn duplicate(image: &RgbaImage, budget: &Budget) -> Option<RgbaImage> {
    budget
        .reserve(image.width(), image.height())
        .then(|| image.clone())
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

/// Largest image, in pixels, the translation derives from source textures: the decode allocation
/// limit at four bytes per pixel, enough for an 8192x8192 atlas from a 512x pack.
const MAX_DERIVED_PIXELS: u64 = MAX_DECODE_BYTES / 4;

/// A transparent `width` x `height` image, or `None` when it exceeds the per-image limit or the
/// translation's budget, so small hostile sources cannot request huge canvases.
pub(crate) fn canvas(width: u32, height: u32, budget: &Budget) -> Option<RgbaImage> {
    budget
        .reserve(width, height)
        .then(|| RgbaImage::new(width, height))
}

/// Whether every pixel is fully transparent.
pub(crate) fn is_empty(image: &RgbaImage) -> bool {
    image.pixels().all(|pixel| pixel.0[3] == 0)
}

/// Copies the `width` x `height` region at (`x`, `y`); parts outside the image stay transparent.
/// Returns `None` when the region exceeds the budget.
pub(crate) fn crop(
    image: &RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    budget: &Budget,
) -> Option<RgbaImage> {
    let mut out = canvas(width, height, budget)?;
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
    Some(out)
}

/// The side of the top square frame when the image is a vertical animation strip.
pub(crate) fn strip_frame(width: u32, height: u32) -> Option<u32> {
    (width != 0 && height >= 2 * width && height.is_multiple_of(width)).then_some(width)
}

/// Multiplies every pixel's colour by `color`, the way Minecraft tints item layers.
pub(crate) fn tint(image: &RgbaImage, color: [u8; 3], budget: &Budget) -> Option<RgbaImage> {
    let mut out = duplicate(image, budget)?;
    for pixel in out.pixels_mut() {
        for (channel, factor) in pixel.0.iter_mut().zip(color) {
            *channel = (u16::from(*channel) * u16::from(factor) / 255) as u8;
        }
    }
    Some(out)
}

/// Composites `top` over `base`, both aligned at their top-left corners, with source-over blending
/// on 16-bit premultiplied values.
pub(crate) fn over(base: &RgbaImage, top: &RgbaImage, budget: &Budget) -> Option<RgbaImage> {
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
    let mut out = duplicate(base, budget)?;
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
    Some(out)
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
        let cropped = crop(&image, 1, 1, 3, 3, &Budget::new()).unwrap();
        assert_eq!(cropped.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(cropped.get_pixel(1, 0).0, [0; 4]);
        assert_eq!(cropped.get_pixel(0, 2).0, [0; 4]);
    }

    #[test]
    fn derived_images_stay_within_the_pixel_budget() {
        let budget = Budget::new();
        assert!(canvas(8192, 8192, &budget).is_some());
        assert!(!budget.exhausted());
        assert!(canvas(279_552, 279_552, &budget).is_none());
        assert!(crop(&RgbaImage::new(1, 1), 0, 0, 1 << 20, 1 << 20, &budget).is_none());
        assert!(
            !budget.exhausted(),
            "an oversized image is refused on its own"
        );
    }

    #[test]
    fn the_budget_bounds_the_total_across_images() {
        let budget = Budget::with_pixels(300);
        assert!(canvas(16, 16, &budget).is_some());
        assert!(canvas(16, 16, &budget).is_none());
        assert!(budget.exhausted());
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
        let out = over(&base, &top, &Budget::new()).unwrap();
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
