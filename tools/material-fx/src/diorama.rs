//! The ship in a bottle's voxel world: a floating island with a village and a waterfall in a
//! 16×16×16 grid, packed into a 64×64 texture of sixteen 16×16 slices (slice `y` at tile
//! `(y % 4, y / 4)`). Each texel's colour is its voxel's; its alpha is the voxel's Chebyshev
//! distance to the nearest solid voxel in voxels over 15, zero for solid voxels, which lets the
//! shader leap across empty space.

use image::{Rgba, RgbaImage};

/// Voxels per grid edge.
pub const SIDE: usize = 16;
/// The largest distance the alpha channel stores.
const MAX_DISTANCE: u8 = 15;

const GRASS: [u8; 3] = [95, 159, 53];
const DIRT: [u8; 3] = [121, 85, 58];
const STONE: [u8; 3] = [125, 125, 128];
const PLANKS: [u8; 3] = [168, 132, 80];
const ROOF: [u8; 3] = [150, 58, 40];
const DOOR: [u8; 3] = [80, 52, 30];
const LANTERN: [u8; 3] = [255, 225, 110];
const WATER: [u8; 3] = [50, 100, 225];
const TRUNK: [u8; 3] = [100, 70, 40];
const LEAVES: [u8; 3] = [60, 125, 42];
const CLOUD: [u8; 3] = [240, 244, 250];

/// The voxel grid, `None` for empty, indexed `[y][z][x]`.
pub type Grid = [[[Option<[u8; 3]>; SIDE]; SIDE]; SIDE];

/// Builds the diorama.
pub fn grid() -> Grid {
    let mut grid: Grid = [[[None; SIDE]; SIDE]; SIDE];
    let mut set = |x: usize, y: usize, z: usize, colour: [u8; 3]| grid[y][z][x] = Some(colour);
    // The island: a grassy disc over dirt, tapering to a stone point underneath.
    for y in 1..=7 {
        let radius = match y {
            7 | 6 => 5.6,
            5 => 5.0,
            4 => 4.0,
            3 => 3.0,
            2 => 2.0,
            _ => 1.0,
        };
        for z in 0..SIDE {
            for x in 0..SIDE {
                let (dx, dz) = (x as f32 - 7.5, z as f32 - 7.5);
                if dx * dx + dz * dz <= radius * radius {
                    let colour = match y {
                        7 => GRASS,
                        5 | 6 => DIRT,
                        _ => STONE,
                    };
                    set(x, y, z, colour);
                }
            }
        }
    }
    // A cottage: plank walls, a door, a lantern window and a pitched roof.
    for y in 8..=9 {
        for z in 4..=7 {
            for x in 4..=7 {
                if x == 4 || x == 7 || z == 4 || z == 7 {
                    set(x, y, z, PLANKS);
                }
            }
        }
    }
    set(5, 8, 7, DOOR);
    set(6, 9, 7, LANTERN);
    for x in 3..=8 {
        for z in 4..=7 {
            set(x, 10, z, ROOF);
        }
        for z in 5..=6 {
            set(x, 11, z, ROOF);
        }
    }
    // A hut beside it.
    for y in 8..=8 {
        for z in 9..=10 {
            for x in 8..=9 {
                set(x, y, z, PLANKS);
            }
        }
    }
    for z in 9..=10 {
        for x in 8..=9 {
            set(x, 9, z, ROOF);
        }
    }
    set(9, 8, 10, LANTERN);
    // A tree.
    for y in 8..=9 {
        set(10, y, 5, TRUNK);
    }
    for y in 10..=11 {
        for z in 4..=6 {
            for x in 9..=11 {
                if y == 10 || (x == 10 || z == 5) {
                    set(x, y, z, LEAVES);
                }
            }
        }
    }
    // A stream across the grass that spills off the edge as a waterfall into a pool.
    for z in 8..=13 {
        set(12, 7, z, WATER);
    }
    for y in 1..=7 {
        set(12, y, 14, WATER);
    }
    for z in 13..=15 {
        for x in 11..=13 {
            set(x, 0, z, WATER);
        }
    }
    // A cloud.
    for x in 1..=3 {
        set(x, 13, 2, CLOUD);
    }
    set(2, 14, 2, CLOUD);
    set(2, 13, 3, CLOUD);
    grid
}

/// Each voxel's Chebyshev distance to the nearest solid voxel, capped at 15, zero inside.
pub fn distances(grid: &Grid) -> Vec<u8> {
    let solids: Vec<(i32, i32, i32)> = (0..SIDE)
        .flat_map(|y| (0..SIDE).flat_map(move |z| (0..SIDE).map(move |x| (x, y, z))))
        .filter(|&(x, y, z)| grid[y][z][x].is_some())
        .map(|(x, y, z)| (x as i32, y as i32, z as i32))
        .collect();
    let mut out = vec![0u8; SIDE * SIDE * SIDE];
    for y in 0..SIDE {
        for z in 0..SIDE {
            for x in 0..SIDE {
                let (px, py, pz) = (x as i32, y as i32, z as i32);
                let nearest = solids
                    .iter()
                    .map(|&(sx, sy, sz)| (sx - px).abs().max((sy - py).abs()).max((sz - pz).abs()))
                    .min()
                    .unwrap_or(i32::from(MAX_DISTANCE));
                out[(y * SIDE + z) * SIDE + x] = nearest.min(i32::from(MAX_DISTANCE)) as u8;
            }
        }
    }
    out
}

/// The diorama as the bottle's texture.
pub fn image(grid: &Grid) -> RgbaImage {
    let distance = distances(grid);
    RgbaImage::from_fn(64, 64, |tx, ty| {
        let (tile_x, tile_y) = (tx as usize / SIDE, ty as usize / SIDE);
        let y = tile_y * 4 + tile_x;
        let (x, z) = (tx as usize % SIDE, ty as usize % SIDE);
        let reach = distance[(y * SIDE + z) * SIDE + x];
        let [r, g, b] = grid[y][z][x].unwrap_or([0, 0, 0]);
        Rgba([
            r,
            g,
            b,
            (u32::from(reach) * 255 / u32::from(MAX_DISTANCE)) as u8,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_voxels_store_zero_and_empty_ones_their_distance() {
        let grid = grid();
        let distance = distances(&grid);
        let at = |x: usize, y: usize, z: usize| distance[(y * SIDE + z) * SIDE + x];
        assert_eq!(at(8, 7, 8), 0, "the island's grass is solid");
        assert_eq!(at(8, 8, 8), 1, "the air above it is one voxel away");
        assert!(at(15, 15, 15) > 1, "the far corner is open sky");
        let image = image(&grid);
        // Slice 7 sits at tile (3, 1); its voxel (8, 8) is grass.
        let texel = image.get_pixel(3 * 16 + 8, 16 + 8);
        assert_eq!((texel[1], texel[3]), (GRASS[1], 0));
    }
}
