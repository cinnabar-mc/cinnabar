//! The showcase's original pixel pictures: the living painting, the rune carving's height map,
//! the liquid metal's skybox, the lenticular mob's three skins and every block's Bedrock look.
//! Each is computed from fixed rules, so equal builds give equal bundles.

use image::{Rgba, RgbaImage};

/// Painting alpha tags the shader animates: plain, sky, eye white, star and grass.
const PLAIN: u8 = 255;
const SKY: u8 = 204;
const EYE: u8 = 153;
const STAR: u8 = 102;
const GRASS: u8 = 51;

/// A 32-bit hash of a point and a seed.
fn hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^ (h >> 13)
}

/// The living painting, 32×32, top row first: an owl on a branch under a moonlit sky.
pub fn painting() -> RgbaImage {
    RgbaImage::from_fn(32, 32, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let eye = |cx: i32| (x - cx).abs() <= 1 && (y - 10).abs() <= 1;
        if eye(13) || eye(19) {
            return Rgba([255, 216, 77, EYE]);
        }
        if x == 16 && (12..=13).contains(&y) {
            return Rgba([235, 140, 40, PLAIN]);
        }
        // The owl: ear tufts, a round brown body with a pale barred belly.
        let (dx, dy) = (x - 16, y - 13);
        let tuft = (y == 6 || y == 7)
            && (x == 11 || x == 12 || x == 20 || x == 21)
            && !(y == 6 && (x == 12 || x == 20));
        let body = dx * dx * 4 + dy * dy * 3 <= 100 && (7..=19).contains(&y);
        if tuft || body {
            let belly = dx.abs() <= 2 && y >= 14;
            if belly {
                let bar = y % 2 == 0;
                return if bar {
                    Rgba([170, 140, 100, PLAIN])
                } else {
                    Rgba([215, 190, 150, PLAIN])
                };
            }
            let feather = (x + y) % 3 == 0;
            return if feather {
                Rgba([90, 62, 40, PLAIN])
            } else {
                Rgba([120, 86, 56, PLAIN])
            };
        }
        // The branch, and its leaves.
        if (19..=20).contains(&y) && (1..=26).contains(&x) {
            return Rgba([95, 65, 38, PLAIN]);
        }
        if y == 18 && (x == 4 || x == 24) || y == 21 && x == 8 {
            return Rgba([60, 110, 50, PLAIN]);
        }
        // Grass and hills.
        if y >= 27 {
            let blade = hash(x, y, 3).is_multiple_of(3);
            return if blade {
                Rgba([70, 120, 55, GRASS])
            } else {
                Rgba([40, 80, 40, GRASS])
            };
        }
        let hill = 23 - ((x as f32 * 0.35).sin() * 2.0) as i32;
        if y >= hill {
            return Rgba([28, 50, 45, PLAIN]);
        }
        // The moon.
        let (mx, my) = (x - 25, y - 5);
        if mx * mx + my * my <= 9 {
            let crater = (mx == 1 && my == -1) || (mx == -1 && my == 1);
            return if crater {
                Rgba([205, 205, 185, PLAIN])
            } else {
                Rgba([240, 238, 215, PLAIN])
            };
        }
        if hash(x, y, 7).is_multiple_of(29) {
            return Rgba([230, 230, 255, STAR]);
        }
        let depth = (y * 4 / 27) as u8;
        Rgba([12 + depth * 4, 18 + depth * 6, 50 + depth * 10, SKY])
    })
}

/// The rune carving's 16×16 height map in alpha: 255 the dressed face, 0 the bottom of a cut,
/// with a sloped step into each groove.
pub fn rune() -> RgbaImage {
    // Two runes, drawn as strokes on the 16×16 grid.
    let strokes: [((i32, i32), (i32, i32)); 7] = [
        ((4, 2), (4, 13)),
        ((4, 2), (8, 6)),
        ((4, 7), (8, 11)),
        ((11, 2), (11, 13)),
        ((11, 6), (14, 3)),
        ((11, 6), (14, 9)),
        ((2, 14), (13, 14)),
    ];
    let on_stroke = |x: i32, y: i32| {
        strokes.iter().any(|&((x0, y0), (x1, y1))| {
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            (0..=steps).any(|i| {
                let px = x0 + (x1 - x0) * i / steps;
                let py = y0 + (y1 - y0) * i / steps;
                px == x && py == y
            })
        })
    };
    RgbaImage::from_fn(16, 16, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let height = if on_stroke(x, y) {
            0
        } else if (-1..=1).any(|dx| (-1..=1).any(|dy| on_stroke(x + dx, y + dy))) {
            150
        } else {
            255
        };
        Rgba([height, height, height, height])
    })
}

/// The liquid metal's 64×32 equirectangular pixel skybox, zenith on top: a banded sunset sky,
/// a sun, clouds and a dark ground with a horizon glow.
pub fn skybox() -> RgbaImage {
    RgbaImage::from_fn(64, 32, |x, y| {
        let (x, y) = (x as i32, y as i32);
        // A bright horizon band, so vertical chrome faces catch the sunset.
        if (13..=18).contains(&y) {
            let warm = (18 - (y - 15).abs() * 3) as u8 * 6;
            return Rgba([200 + warm / 2, 120 + warm, 90 + warm / 3, 255]);
        }
        if y > 18 {
            let tile = ((x / 4 + y / 2) % 2) as u8 * 25;
            return Rgba([90 + tile, 70 + tile, 60 + tile, 255]);
        }
        if (x - 40).abs() <= 2 && (y - 10).abs() <= 2 {
            return Rgba([255, 245, 190, 255]);
        }
        if (y == 6 || y == 7) && ((x / 5) % 3 == 0) && x > 4 {
            return Rgba([250, 210, 200, 255]);
        }
        let band = (y / 3) as u8;
        Rgba([60 + band * 25, 90 + band * 15, 200 - band * 10, 255])
    })
}

/// The lenticular mob's three 64×64 skins side by side, in the humanoid skin layout: a grinning
/// gold mask, a creeping green face and a bone-white skull, each on matching clothes.
pub fn lenticular() -> RgbaImage {
    let palettes = [
        ([235, 180, 40], [120, 60, 20], [40, 20, 10]),
        ([90, 190, 80], [40, 110, 40], [10, 30, 10]),
        ([235, 235, 225], [150, 150, 160], [20, 20, 30]),
    ];
    RgbaImage::from_fn(192, 64, |x, y| {
        let skin = (x / 64) as usize;
        let (x, y) = ((x % 64) as i32, y as i32);
        let (face, cloth, mark) = palettes[skin];
        let in_head_front = (8..16).contains(&x) && (8..16).contains(&y);
        if in_head_front {
            let (fx, fy) = (x - 8, y - 8);
            let feature = match skin {
                0 => {
                    (fy == 3 && (fx == 2 || fx == 5))
                        || (fy == 5 && (1..=6).contains(&fx))
                        || (fy == 6 && (2..=5).contains(&fx))
                }
                1 => {
                    ((fy == 2 || fy == 3) && (fx == 1 || fx == 2 || fx == 5 || fx == 6))
                        || ((4..=7).contains(&fy) && (3..=4).contains(&fx))
                        || (fy >= 5 && (fx == 2 || fx == 5))
                }
                _ => {
                    ((2..=3).contains(&fy) && (1..=2).contains(&fx)
                        || (2..=3).contains(&fy) && (5..=6).contains(&fx))
                        || (fy == 5 && fx == 3)
                        || (fy == 6 && fx % 2 == 1)
                }
            };
            let [r, g, b] = if feature { mark } else { face };
            return Rgba([r, g, b, 255]);
        }
        let head = y < 16;
        let [r, g, b] = if head { face } else { cloth };
        let shade = if (x + y) % 4 == 0 { 20 } else { 0 };
        Rgba([
            r.saturating_sub(shade),
            g.saturating_sub(shade),
            b.saturating_sub(shade),
            255,
        ])
    })
}

/// A block's Bedrock look while its material is revoked: a 16×16 pattern in two colours.
pub fn block_look(base: [u8; 3], accent: [u8; 3], seed: u32) -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        let edge = x == 0 || y == 0 || x == 15 || y == 15;
        let speck = hash(x as i32, y as i32, seed).is_multiple_of(5);
        let [r, g, b] = if edge || speck { accent } else { base };
        Rgba([r, g, b, 255])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pictures_are_deterministic_and_tag_their_animated_pixels() {
        assert_eq!(painting(), painting());
        let painting = painting();
        // Both eyes are 3×3 whites around (13, 10) and (19, 10).
        for cx in [13, 19] {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    assert_eq!(
                        painting.get_pixel((cx + dx) as u32, (10 + dy) as u32)[3],
                        EYE
                    );
                }
            }
        }
        assert!(painting.pixels().any(|pixel| pixel[3] == STAR));
        assert!(painting.pixels().any(|pixel| pixel[3] == SKY));
        let rune = rune();
        assert!(
            rune.pixels().any(|pixel| pixel[3] == 0) && rune.pixels().any(|pixel| pixel[3] == 255)
        );
        assert_eq!(lenticular().dimensions(), (192, 64));
        assert_eq!(skybox().dimensions(), (64, 32));
    }
}
