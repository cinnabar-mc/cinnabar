//! Vault interiors: inward quads just inside the cutout shell
//! (`BlockTessellator::tessellateVaultInWorld`).

use {super::super::*, assets::BlockFace};

/// Inner faces 0.01 blocks inside the shell, rounded to model units.
const NEAR: i16 = 3;
const FAR: i16 = 253;
/// The interior floor height (3/16).
const FLOOR: i16 = 48;

/// The six inward quads, indexed like `materials` and `flips` by [`BlockFace`]. Each draws
/// the material of the shell face behind it. The ceiling and floor turn by the native
/// remapping of the flip of the shell face they look toward; the walls mirror their texture
/// horizontally. Inner quads never cull.
pub(in crate::compiler) fn inner_quads(materials: [u32; 6], flips: [u8; 6]) -> [ModelQuad; 6] {
    const INWARD: [u8; 6] = [3, 2, 1, 0, 0, 3];
    let inward_flip = |face: BlockFace| {
        INWARD
            .get(usize::from(flips[face as usize]))
            .copied()
            .unwrap_or(0)
    };
    // (face drawn, shell face behind it, min, max, flip)
    let parts = [
        (
            BlockFace::Down,
            BlockFace::Up,
            [NEAR, FAR, NEAR],
            [FAR, FAR + 1, FAR],
            inward_flip(BlockFace::Down),
        ),
        (
            BlockFace::Up,
            BlockFace::Down,
            [0, FLOOR - 1, 0],
            [256, FLOOR, FAR],
            inward_flip(BlockFace::Up),
        ),
        (
            BlockFace::North,
            BlockFace::South,
            [NEAR, FLOOR, FAR],
            [FAR, FAR, FAR + 1],
            super::geometry::FLIP_MIRROR,
        ),
        (
            BlockFace::South,
            BlockFace::North,
            [NEAR, FLOOR, NEAR - 1],
            [FAR, FAR, NEAR],
            super::geometry::FLIP_MIRROR,
        ),
        (
            BlockFace::West,
            BlockFace::East,
            [FAR, FLOOR, NEAR],
            [FAR + 1, FAR, FAR],
            super::geometry::FLIP_MIRROR,
        ),
        (
            BlockFace::East,
            BlockFace::West,
            [NEAR - 1, FLOOR, NEAR],
            [NEAR, FAR, FAR],
            super::geometry::FLIP_MIRROR,
        ),
    ];
    parts.map(|(face, behind, min, max, flip)| {
        let mut quad =
            super::geometry::vanilla_cuboid_quads([materials[behind as usize]; 6], min, max)
                [face as usize];
        super::geometry::flip_uvs(&mut quad, flip);
        quad
    })
}
