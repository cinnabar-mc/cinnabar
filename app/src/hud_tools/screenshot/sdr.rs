//! Frame captures as 8-bit sRGB. A readback of an HDR swapchain holds the HDR signal itself, so
//! it is decoded with the paper white it was encoded with and its highlights are clipped, the
//! way the same frame looks on an SDR display.

use std::sync::LazyLock;

use bevy::prelude::Image;
use image::RgbImage;
use render_api::hdr::{HdrEncoding, sdr_srgb8};

/// The capture as 8-bit sRGB RGB. An HDR swapchain readback is decoded with
/// `paper_white_nits`, the level it encoded SDR white at; SDR readbacks convert as they are.
pub(crate) fn rgb8(image: Image, paper_white_nits: f32) -> Result<RgbImage, String> {
    let Some(hdr) = HdrReadback::new(&image) else {
        return image
            .try_into_dynamic()
            .map(|dynamic| dynamic.to_rgb8())
            .map_err(|error| error.to_string());
    };
    let mut pixels = vec![0; hdr.pixels() * 3];
    hdr.write_sdr::<3>(paper_white_nits, &mut pixels)?;
    RgbImage::from_raw(image.width(), image.height(), pixels)
        .ok_or_else(|| "the capture's pixels do not fill its size".to_owned())
}

/// A readback of a window presenting HDR, recognised by the swapchain format that carries it.
pub(crate) struct HdrReadback<'a> {
    image: &'a Image,
    encoding: HdrEncoding,
}

impl<'a> HdrReadback<'a> {
    /// `None` for SDR readbacks, whose pixels are already sRGB.
    pub(crate) fn new(image: &'a Image) -> Option<Self> {
        render::output_encoding(image.texture_descriptor.format)
            .map(|encoding| Self { image, encoding })
    }

    /// Pixels in the capture.
    pub(crate) fn pixels(&self) -> usize {
        self.image.width() as usize * self.image.height() as usize
    }

    /// Writes the capture as tightly packed 8-bit sRGB with `CHANNELS` bytes per pixel, RGB or
    /// RGBA with opaque alpha. Each colour byte is
    /// `sdr_srgb8(encoding.decode(texel, paper_white_nits))`.
    pub(crate) fn write_sdr<const CHANNELS: usize>(
        &self,
        paper_white_nits: f32,
        out: &mut [u8],
    ) -> Result<(), String> {
        const { assert!(CHANNELS == 3 || CHANNELS == 4) };
        let texels = self
            .image
            .data
            .as_deref()
            .ok_or("the capture has no pixel data")?;
        let decoder = Decoder::new(self.encoding, paper_white_nits);
        let texel_bytes = decoder.texel_bytes();
        let pixels = self.pixels();
        if texels.len() != pixels * texel_bytes || out.len() != pixels * CHANNELS {
            return Err(format!(
                "a {:?} capture of {pixels} pixels has {} bytes",
                self.image.texture_descriptor.format,
                texels.len()
            ));
        }
        let levels: &[f32; 256] = &SRGB_LEVELS;
        for (texel, pixel) in texels
            .chunks_exact(texel_bytes)
            .zip(out.as_chunks_mut::<CHANNELS>().0)
        {
            for (byte, linear) in pixel.iter_mut().zip(decoder.linear(texel)) {
                *byte = srgb_byte(levels, linear);
            }
            if CHANNELS == 4 {
                pixel[3] = u8::MAX;
            }
        }
        Ok(())
    }
}

/// Distinct codes of a 10-bit PQ channel.
const PQ_CODES: usize = 1 << 10;

/// Decodes one texel to linear BT.709 relative to paper white, exactly as
/// [`HdrEncoding::decode`] does.
enum Decoder {
    /// `Rgba16Float`: linear half floats, scaled per texel.
    Scrgb { paper_white_nits: f32 },
    /// `Rgb10a2Unorm`: PQ applies its transfer per channel before one linear primaries
    /// conversion, so a texel decodes as the sum of its channels decoded alone, tabulated
    /// per 10-bit code instead of evaluating the transfer for every pixel.
    Pq(Box<[[[f32; 3]; PQ_CODES]; 3]>),
}

impl Decoder {
    fn new(encoding: HdrEncoding, paper_white_nits: f32) -> Self {
        match encoding {
            HdrEncoding::Scrgb => Self::Scrgb { paper_white_nits },
            HdrEncoding::Pq => Self::Pq(Box::new(std::array::from_fn(|channel| {
                std::array::from_fn(|code| {
                    let mut texel = [0.0; 3];
                    texel[channel] = pq_signal(code as u32);
                    encoding.decode(texel, paper_white_nits)
                })
            }))),
        }
    }

    /// Bytes of one texel in the swapchain format carrying this encoding.
    fn texel_bytes(&self) -> usize {
        match self {
            Self::Scrgb { .. } => 8,
            Self::Pq(_) => 4,
        }
    }

    /// `texel` holds [`Self::texel_bytes`] little-endian bytes.
    fn linear(&self, texel: &[u8]) -> [f32; 3] {
        match self {
            Self::Scrgb { paper_white_nits } => HdrEncoding::Scrgb.decode(
                std::array::from_fn(|channel| {
                    half::f16::from_le_bytes([texel[2 * channel], texel[2 * channel + 1]]).to_f32()
                }),
                *paper_white_nits,
            ),
            Self::Pq(channels) => {
                let packed = u32::from_le_bytes([texel[0], texel[1], texel[2], texel[3]]);
                let [r, g, b] = std::array::from_fn(|channel| {
                    &channels[channel][(packed >> (10 * channel)) as usize & (PQ_CODES - 1)]
                });
                std::array::from_fn(|row| r[row] + g[row] + b[row])
            }
        }
    }
}

/// A 10-bit unorm code as its PQ signal in `0..=1`.
fn pq_signal(code: u32) -> f32 {
    code as f32 / (PQ_CODES - 1) as f32
}

/// The least linear level [`sdr_srgb8`] encodes as each byte, found by bisecting `f32` bit
/// patterns (ordered like their values when non-negative), so a channel encodes by a short
/// search instead of a `powf`.
static SRGB_LEVELS: LazyLock<[f32; 256]> = LazyLock::new(|| {
    std::array::from_fn(|byte| {
        if byte == 0 {
            return f32::NEG_INFINITY;
        }
        let reaches =
            |bits: u32| usize::from(sdr_srgb8([f32::from_bits(bits), 0.0, 0.0])[0]) >= byte;
        let (mut below, mut at) = (0.0_f32.to_bits(), 1.0_f32.to_bits());
        while at - below > 1 {
            let middle = below + (at - below) / 2;
            if reaches(middle) {
                at = middle;
            } else {
                below = middle;
            }
        }
        f32::from_bits(at)
    })
});

/// The byte [`sdr_srgb8`] encodes `linear` as; NaN, like there, encodes as zero.
fn srgb_byte(levels: &[f32; 256], linear: f32) -> u8 {
    let reached = levels.partition_point(|&level| level <= linear);
    u8::try_from(reached.saturating_sub(1)).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests;
