//! The showcase's original generated art: every image is computed from a fixed seed, so equal
//! builds give equal bundles and no image is ever committed.

use image::{ImageFormat, Rgba, RgbaImage};

/// A 32-bit integer hash of a lattice point and a seed.
fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

/// A lattice value in `[0, 1]`, wrapping every `period` cells so the result tiles.
fn lattice(x: i32, y: i32, period: i32, seed: u32) -> f32 {
    (hash(x.rem_euclid(period), y.rem_euclid(period), seed) & 0xffff) as f32 / 65535.0
}

/// Tileable value noise at `(u, v)` in cells, smoothly interpolated.
fn value_noise(u: f32, v: f32, period: i32, seed: u32) -> f32 {
    let (x, y) = (u.floor() as i32, v.floor() as i32);
    let (fx, fy) = (u - u.floor(), v - v.floor());
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let top = lattice(x, y, period, seed) * (1.0 - sx) + lattice(x + 1, y, period, seed) * sx;
    let bottom =
        lattice(x, y + 1, period, seed) * (1.0 - sx) + lattice(x + 1, y + 1, period, seed) * sx;
    top * (1.0 - sy) + bottom * sy
}

/// Four octaves of tileable noise over a `side`-texel square, normalized to `[0, 1]`.
fn fractal(side: u32, seed: u32) -> Vec<f32> {
    let mut values = Vec::with_capacity((side * side) as usize);
    for y in 0..side {
        for x in 0..side {
            let mut total = 0.0;
            let mut weight = 0.0;
            for octave in 0..4 {
                let cells = 4 << octave;
                let scale = cells as f32 / side as f32;
                let amplitude = 0.5_f32.powi(octave);
                total += value_noise(
                    x as f32 * scale,
                    y as f32 * scale,
                    cells,
                    seed + octave as u32,
                ) * amplitude;
                weight += amplitude;
            }
            values.push(total / weight);
        }
    }
    let (low, high) = values.iter().fold((f32::MAX, f32::MIN), |(low, high), &v| {
        (low.min(v), high.max(v))
    });
    values
        .into_iter()
        .map(|value| (value - low) / (high - low).max(f32::EPSILON))
        .collect()
}

/// `image` as PNG bytes.
fn png(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("an in-memory PNG always encodes");
    bytes.into_inner()
}

/// The dissolve's 64×64 tileable grey value noise.
pub fn noise() -> Vec<u8> {
    let values = fractal(64, 11);
    png(&RgbaImage::from_fn(64, 64, |x, y| {
        let level = (values[(y * 64 + x) as usize] * 255.0).round() as u8;
        Rgba([level, level, level, 255])
    }))
}

/// The portal's 64×64 starfield: sparse grey stars of varied brightness on black.
pub fn stars() -> Vec<u8> {
    png(&RgbaImage::from_fn(64, 64, |x, y| {
        let h = hash(x as i32, y as i32, 29);
        let level = if h.is_multiple_of(23) {
            96 + (h >> 24) as u8 % 160
        } else {
            0
        };
        Rgba([level, level, level, 255])
    }))
}

/// The energy conduit's Bedrock look, 16×16: dark basalt with faint orange veins.
pub fn energy_block() -> Vec<u8> {
    let values = fractal(16, 41);
    png(&RgbaImage::from_fn(16, 16, |x, y| {
        let vein = values[(y * 16 + x) as usize];
        if vein > 0.72 {
            Rgba([160, 70, 20, 255])
        } else {
            let level = 40 + (vein * 30.0) as u8;
            Rgba([level, level, level + 4, 255])
        }
    }))
}

/// The star portal's Bedrock look, 16×16: deep violet stone.
pub fn portal_block() -> Vec<u8> {
    let values = fractal(16, 53);
    png(&RgbaImage::from_fn(16, 16, |x, y| {
        let level = values[(y * 16 + x) as usize];
        Rgba([
            30 + (level * 25.0) as u8,
            10 + (level * 10.0) as u8,
            50 + (level * 40.0) as u8,
            255,
        ])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory(bytes).unwrap().into_rgba8()
    }

    #[test]
    fn the_art_is_deterministic_and_the_noise_tiles_across_its_range() {
        assert_eq!(noise(), noise());
        assert_eq!(stars(), stars());
        let noise = decode(&noise());
        let levels: Vec<u8> = noise.pixels().map(|pixel| pixel[0]).collect();
        assert_eq!(levels.iter().min(), Some(&0));
        assert_eq!(levels.iter().max(), Some(&255));
        // Opposite edges are lattice neighbours, so the noise has no seam where it wraps.
        let seam = (0..64)
            .map(|y| i32::from(noise.get_pixel(0, y)[0]) - i32::from(noise.get_pixel(63, y)[0]))
            .map(i32::abs)
            .max()
            .unwrap();
        assert!(seam < 40, "{seam}");
        let stars = decode(&stars());
        let lit = stars.pixels().filter(|pixel| pixel[0] > 0).count();
        assert!((40..400).contains(&lit), "{lit}");
    }
}
