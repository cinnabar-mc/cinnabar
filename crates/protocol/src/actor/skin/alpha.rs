//! Skin alpha normalization at the network and local-import boundaries.

use render_api::{CLASSIC_SKIN_SIDE, MAX_CLASSIC_SKIN_SIDE};

/// Binarizes received non-persona texels while preserving authored holes in every model.
/// Classic body-coverage protection belongs to local imports, not received appearances.
pub(super) fn normalize(is_persona: bool, width: u32, height: u32, pixels: &mut [u8]) {
    if !is_persona {
        normalize_pixels(width, height, pixels, false);
    }
}

/// Validates a local classic import and applies wide/slim alpha and body-coverage rules.
pub fn normalize_classic_skin_rgba8(width: u32, height: u32, pixels: &mut [u8]) -> bool {
    normalize_pixels(width, height, pixels, true)
}

/// Binarizes custom skin texels without forcing humanoid body coverage.
pub fn normalize_custom_skin_rgba8(width: u32, height: u32, pixels: &mut [u8]) -> bool {
    normalize_pixels(width, height, pixels, false)
}

fn normalize_pixels(width: u32, height: u32, pixels: &mut [u8], protect_body: bool) -> bool {
    if width != height
        || !(width as usize == CLASSIC_SKIN_SIDE || width as usize == MAX_CLASSIC_SKIN_SIDE)
        || pixels.len() != width as usize * height as usize * 4
    {
        return false;
    }
    let side = width as usize;
    let scale = side / CLASSIC_SKIN_SIDE;
    // Vanilla validates alpha and coverage across these skin regions.
    for (bounds, protect) in [
        ([0, 8, 32, 16], true),
        ([8, 0, 24, 8], false),
        ([0, 20, 56, 32], true),
        ([4, 16, 12, 20], false),
        ([20, 16, 36, 20], false),
        ([44, 16, 52, 20], false),
        ([16, 52, 48, 64], true),
        ([20, 48, 28, 64], false),
        ([36, 48, 44, 64], false),
        ([32, 0, 64, 32], false),
        ([0, 32, 16, 48], false),
        ([16, 32, 40, 48], false),
        ([40, 32, 56, 48], false),
        ([0, 48, 16, 64], false),
        ([48, 48, 64, 64], false),
    ] {
        region(
            pixels,
            side,
            bounds.map(|value| value * scale),
            protect && protect_body,
        );
    }
    true
}

/// Applies the 26/255 alpha cutoff and the strict 60% transparent coverage threshold.
fn region(pixels: &mut [u8], side: usize, [x0, y0, x1, y1]: [usize; 4], protect: bool) {
    let mut transparent = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let alpha = &mut pixels[(y * side + x) * 4 + 3];
            transparent += usize::from(*alpha < 26);
            *alpha = if *alpha < 26 { 0 } else { 255 };
        }
    }
    if protect && transparent as f32 / ((x1 - x0) * (y1 - y0)) as f32 > 0.6 {
        for y in y0..y1 {
            for x in x0..x1 {
                pixels[(y * side + x) * 4 + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_classic_body_sides_become_visible_without_filling_the_hat() {
        for side in [CLASSIC_SKIN_SIDE as u32, MAX_CLASSIC_SKIN_SIDE as u32] {
            let mut pixels = vec![0; (side * side * 4) as usize];
            assert!(normalize_classic_skin_rgba8(side, side, &mut pixels));
            let scale = side as usize / CLASSIC_SKIN_SIDE;
            assert_eq!(pixels[((8 * scale * side as usize) * 4) + 3], 255);
            assert_eq!(pixels[(32 * scale * 4) + 3], 0);
        }
    }

    #[test]
    fn received_skin_keeps_holes_and_binarizes_partial_alpha() {
        let side = MAX_CLASSIC_SKIN_SIDE as u32;
        let mut pixels = vec![0; (side * side * 4) as usize];
        let index = (64 * 4) + 3;
        pixels[index] = 56;
        normalize(false, side, side, &mut pixels);
        assert_eq!(pixels[index], 255);
        assert_eq!(pixels[16 * MAX_CLASSIC_SKIN_SIDE * 4 + 3], 0);
    }

    #[test]
    fn persona_alpha_is_not_reinterpreted_as_classic() {
        let side = CLASSIC_SKIN_SIDE as u32;
        let mut pixels = vec![0; (side * side * 4) as usize];
        pixels[3] = 56;
        let before = pixels.clone();
        normalize(true, side, side, &mut pixels);
        assert_eq!(pixels, before);
    }
}
