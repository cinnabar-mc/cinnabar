use super::*;
use wgpu::{DisplayCoarseRange, SurfaceColorSpaces, SurfaceFormatCapabilities};

fn capabilities(formats: &[(TextureFormat, SurfaceColorSpaces)]) -> SurfaceCapabilities {
    SurfaceCapabilities {
        formats: formats.iter().map(|(format, _)| *format).collect(),
        format_capabilities: formats
            .iter()
            .map(|&(format, color_spaces)| SurfaceFormatCapabilities {
                format,
                color_spaces,
            })
            .collect(),
        ..SurfaceCapabilities::default()
    }
}

fn display(high_dynamic_range: Option<bool>) -> DisplayHdrInfo {
    DisplayHdrInfo {
        coarse: Some(DisplayCoarseRange {
            high_dynamic_range,
            gamut: None,
        }),
        ..DisplayHdrInfo::default()
    }
}

const SDR: (TextureFormat, SurfaceColorSpaces) =
    (TextureFormat::Bgra8UnormSrgb, SurfaceColorSpaces::SRGB);
const SCRGB: (TextureFormat, SurfaceColorSpaces) = (
    TextureFormat::Rgba16Float,
    SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
);
const HDR10: (TextureFormat, SurfaceColorSpaces) = (
    TextureFormat::Rgb10a2Unorm,
    SurfaceColorSpaces::SRGB.union(SurfaceColorSpaces::BT2100_PQ),
);

#[test]
fn prefers_scrgb_then_hdr10_on_an_hdr_display() {
    let hdr = display(Some(true));
    assert_eq!(
        choose_encoding(&capabilities(&[SDR, HDR10, SCRGB]), &hdr),
        Some(HdrEncoding::Scrgb)
    );
    assert_eq!(
        choose_encoding(&capabilities(&[SDR, HDR10]), &hdr),
        Some(HdrEncoding::Pq)
    );
    assert_eq!(choose_encoding(&capabilities(&[SDR]), &hdr), None);
}

#[test]
fn an_sdr_display_keeps_sdr_even_when_hdr_formats_are_offered() {
    let offered = capabilities(&[SDR, HDR10, SCRGB]);
    assert_eq!(choose_encoding(&offered, &display(Some(false))), None);
    // An unknown display state defers to what the surface offers.
    assert_eq!(
        choose_encoding(&offered, &DisplayHdrInfo::default()),
        Some(HdrEncoding::Scrgb)
    );
    assert_eq!(
        choose_encoding(&offered, &display(None)),
        Some(HdrEncoding::Scrgb)
    );
}

#[test]
fn hdr10_without_its_pq_colour_space_is_not_hdr() {
    let sdr_10_bit = (TextureFormat::Rgb10a2Unorm, SurfaceColorSpaces::SRGB);
    assert_eq!(
        choose_encoding(&capabilities(&[SDR, sdr_10_bit]), &display(Some(true))),
        None
    );
}

#[test]
fn output_formats_identify_their_encoding() {
    for encoding in [HdrEncoding::Scrgb, HdrEncoding::Pq] {
        let (format, space) = swapchain(encoding);
        assert!(space.is_hdr());
        assert_eq!(output_encoding(format), Some(encoding));
    }
    for format in [
        TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8Unorm,
    ] {
        assert_eq!(output_encoding(format), None);
    }
}

#[test]
fn hdr_scenes_keep_their_range_and_sdr_scenes_expand_to_the_boosted_white() {
    let output = HdrOutput {
        enabled: true,
        peak_nits: 1000.0,
        paper_white_nits: 200.0,
        highlight_boost: 0.5,
    };
    assert_eq!(HdrStageConstants::new(output, false).sdr_white, 1.0);
    assert_eq!(HdrStageConstants::new(output, true).sdr_white, 3.0);
    assert_ne!(
        HdrStageConstants::new(output, true).key(),
        HdrStageConstants::new(output, false).key()
    );
}
