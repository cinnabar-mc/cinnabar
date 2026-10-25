use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use half::f16;
use render_api::hdr::SCRGB_REFERENCE_NITS;

use super::*;

const PAPER_WHITE_NITS: f32 = 200.0;

/// One row of `texels` in `format`.
fn row(format: TextureFormat, texels: Vec<u8>) -> Image {
    let texel_bytes = format.block_copy_size(None).unwrap() as usize;
    Image::new(
        Extent3d {
            width: u32::try_from(texels.len() / texel_bytes).unwrap(),
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texels,
        format,
        RenderAssetUsages::default(),
    )
}

/// The scRGB swapchain's half-float texels.
fn scrgb(texels: impl IntoIterator<Item = [f32; 3]>) -> Image {
    let bytes = texels
        .into_iter()
        .flat_map(|[r, g, b]| [r, g, b, 1.0])
        .flat_map(|value| f16::from_f32(value).to_le_bytes())
        .collect();
    row(TextureFormat::Rgba16Float, bytes)
}

/// Greys at multiples of paper white, as the scRGB swapchain carries them.
fn scrgb_grey(paper_whites: &[f32]) -> Image {
    scrgb(
        paper_whites
            .iter()
            .map(|level| [level * PAPER_WHITE_NITS / SCRGB_REFERENCE_NITS; 3]),
    )
}

/// The PQ swapchain's packed 10-bit texels with opaque alpha.
fn pq(codes: impl IntoIterator<Item = [u32; 3]>) -> Image {
    let bytes = codes
        .into_iter()
        .flat_map(|[r, g, b]| (r | g << 10 | b << 20 | 3 << 30).to_le_bytes())
        .collect();
    row(TextureFormat::Rgb10a2Unorm, bytes)
}

fn rgb_pixels(image: Image) -> Vec<[u8; 3]> {
    rgb8(image, PAPER_WHITE_NITS)
        .unwrap()
        .pixels()
        .map(|pixel| pixel.0)
        .collect()
}

#[test]
fn scrgb_paper_white_is_sdr_white_and_highlights_clip() {
    assert_eq!(
        rgb_pixels(scrgb_grey(&[1.0, 4.0, 0.25, 0.0])),
        [[255; 3], [255; 3], [137; 3], [0; 3]]
    );
}

#[test]
fn pq_greys_decode_relative_to_paper_white() {
    // 10-bit PQ codes of 50, 100 and 10,000 nits: a quarter, half and fifty times paper white.
    assert_eq!(
        rgb_pixels(pq([[450; 3], [520; 3], [1023; 3], [0; 3]])),
        [[137; 3], [188; 3], [255; 3], [0; 3]]
    );
}

#[test]
fn tabulated_decoding_matches_the_reference_conversion() {
    let reference =
        |encoding: HdrEncoding, texel| sdr_srgb8(encoding.decode(texel, PAPER_WHITE_NITS));
    let codes: Vec<[u32; 3]> = (0..PQ_CODES as u32)
        .map(|code| [code, (code * 7) % 1024, (code * 13 + 5) % 1024])
        .collect();
    let expected: Vec<_> = codes
        .iter()
        .map(|&codes| reference(HdrEncoding::Pq, codes.map(pq_signal)))
        .collect();
    assert_eq!(rgb_pixels(pq(codes)), expected);

    let texels: Vec<[f32; 3]> = (0..4096_u16)
        .map(|step| {
            let level = f32::from(step) / 256.0 - 1.0;
            [level, level * 0.5, level * level].map(|value| f16::from_f32(value).to_f32())
        })
        .collect();
    let expected: Vec<_> = texels
        .iter()
        .map(|&texel| reference(HdrEncoding::Scrgb, texel))
        .collect();
    assert_eq!(rgb_pixels(scrgb(texels)), expected);
}

#[test]
fn byte_search_matches_srgb_encoding_at_every_level() {
    let levels: &[f32; 256] = &SRGB_LEVELS;
    let samples = (0..=200_000_u32)
        .map(|step| step as f32 / 190_000.0 - 0.01)
        .chain(
            levels[1..]
                .iter()
                .flat_map(|&level| [level, f32::from_bits(level.to_bits() - 1)]),
        )
        .chain([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
    for linear in samples {
        assert_eq!(
            srgb_byte(levels, linear),
            sdr_srgb8([linear, 0.0, 0.0])[0],
            "{linear}"
        );
    }
}

#[test]
fn rgba_output_has_opaque_alpha() {
    let image = scrgb_grey(&[0.25]);
    let mut pixel = [0; 4];
    HdrReadback::new(&image)
        .unwrap()
        .write_sdr::<4>(PAPER_WHITE_NITS, &mut pixel)
        .unwrap();
    assert_eq!(pixel, [137, 137, 137, 255]);
}

#[test]
fn sdr_captures_keep_their_pixels() {
    let image = row(
        TextureFormat::Rgba8UnormSrgb,
        vec![10, 20, 30, 40, 200, 100, 50, 255],
    );
    assert!(HdrReadback::new(&image).is_none());
    assert_eq!(rgb_pixels(image), [[10, 20, 30], [200, 100, 50]]);
}
