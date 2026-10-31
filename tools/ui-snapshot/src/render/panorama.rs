//! CPU composition of the launcher's panorama beneath transparent screen paint.

use assets::RuntimeUiAssets;
use render_model::panorama::{
    PanoramaFaces, PanoramaView,
    cpu::{background, linear_to_srgb},
    launcher_faces, launcher_view, overlay_tint,
};

use crate::carriers::Error;

/// Uses the title screen's faces, camera and tint at the snapshot's fixed time.
pub(super) fn composite(
    pixels: &mut [u8],
    size: [u32; 2],
    assets: &RuntimeUiAssets,
) -> Result<(), Error> {
    let faces = launcher_faces(assets)
        .ok_or_else(|| Error::Missing("title panorama faces; make assets".into()))?;
    let view = launcher_view(
        super::CAPTURE_SECONDS as f32,
        size[0] as f32 / size[1] as f32,
        overlay_tint(assets),
    );
    blend(pixels, size, &faces, view);
    Ok(())
}

/// Composites straight-alpha screen paint over the panorama pass's output.
fn blend(pixels: &mut [u8], size: [u32; 2], faces: &PanoramaFaces, view: PanoramaView) {
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let alpha = f32::from(pixel[3]) / 255.0;
        if alpha == 1.0 {
            continue;
        }
        let x = index as u32 % size[0];
        let y = index as u32 / size[0];
        let tinted = background(faces, &view, size, x, y);
        for channel in 0..3 {
            let srgb = linear_to_srgb(tinted[channel]);
            pixel[channel] =
                (f32::from(pixel[channel]) * alpha + srgb * (1.0 - alpha)).round() as u8;
        }
        pixel[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panorama_composition_preserves_panels_and_fills_transparent_pixels() {
        let faces =
            PanoramaFaces::new(1, std::array::from_fn(|_| vec![120, 160, 200, 255])).unwrap();
        let mut pixels = [0, 0, 0, 0, 12, 34, 56, 255];
        blend(
            &mut pixels,
            [2, 1],
            &faces,
            launcher_view(0.0, 2.0, [0.0; 4]),
        );
        assert_eq!(pixels, [120, 160, 200, 255, 12, 34, 56, 255]);
    }
}
