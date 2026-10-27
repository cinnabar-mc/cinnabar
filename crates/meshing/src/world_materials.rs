//! Blocks a programmable world material shades, as terrain meshing reads them.

/// One block network value a programmable world material shades.
///
/// Meshing takes these sorted by [`Self::network_value`], one per value. A bound block's cube
/// faces carry its tag and draw only in their material's pass; they merge only with faces of
/// the same tag, and not at all when the material [`Self::displaces`] them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldMaterialBlock {
    /// The block's network value.
    pub network_value: u32,
    /// One more than the material slot, at most [`crate::PackedQuad::MAX_WORLD_MATERIAL`].
    pub tag: u8,
    /// Whether the material lets what lies behind the block show (cutout or blended). Such a
    /// block hides no face of a block with another tag or none, as glass hides no stone face,
    /// whatever its Bedrock look; a face against a same-tag block that covers the whole face
    /// stays hidden, as glass hides glass.
    pub see_through: bool,
    /// Whether the material may move the block's vertices. Such a block hides no face at all,
    /// not even a same-tag neighbour's, since its own faces may move away from the face between
    /// them; its faces stay one block wide so every block moves on its own.
    pub displaces: bool,
}

impl WorldMaterialBlock {
    /// A block bound to `tag` whose material lets the world behind it show when `see_through`.
    #[must_use]
    pub const fn new(network_value: u32, tag: u8, see_through: bool) -> Self {
        Self {
            network_value,
            tag,
            see_through,
            displaces: false,
        }
    }

    /// This block, its material moving its vertices when `displaces`.
    #[must_use]
    pub const fn displacing(mut self, displaces: bool) -> Self {
        self.displaces = displaces;
        self
    }
}

/// The entry `blocks` (sorted by network value) holds for `network_value`.
pub(crate) fn find(
    blocks: &[WorldMaterialBlock],
    network_value: u32,
) -> Option<WorldMaterialBlock> {
    if blocks.is_empty() {
        return None;
    }
    blocks
        .binary_search_by_key(&network_value, |block| block.network_value)
        .ok()
        .map(|index| blocks[index])
}

/// One more than the world material slot `blocks` give `network_value`; zero for none.
pub(crate) fn tag(blocks: &[WorldMaterialBlock], network_value: u32) -> u8 {
    find(blocks, network_value).map_or(0, |block| block.tag)
}

/// Whether `blocks` make `network_value` hide no other block's faces: its material is
/// see-through or moves its faces.
pub(crate) fn hides_no_other(blocks: &[WorldMaterialBlock], network_value: u32) -> bool {
    find(blocks, network_value).is_some_and(|block| block.see_through || block.displaces)
}

/// Whether `blocks` let a world material move `network_value`'s vertices.
pub(crate) fn displaces(blocks: &[WorldMaterialBlock], network_value: u32) -> bool {
    find(blocks, network_value).is_some_and(|block| block.displaces)
}

/// Whether blocks `cell` and `neighbour` carry the same world material tag, so the neighbour
/// hides the face between them where it covers that whole face; untagged blocks and blocks
/// whose material moves their faces never match.
pub(crate) fn same_tag(blocks: &[WorldMaterialBlock], cell: u32, neighbour: u32) -> bool {
    find(blocks, cell).is_some_and(|cell| !cell.displaces && cell.tag == tag(blocks, neighbour))
}

impl crate::PackedQuad {
    /// The section blocks whose faces this quad covers, row by row from its origin: its width
    /// runs along the face's first in-plane axis (z for x faces, x otherwise) and its height
    /// along the second (z for y faces, y otherwise).
    pub fn blocks(&self) -> impl Iterator<Item = [u8; 3]> + use<> {
        let (origin, face) = (self.origin(), self.face());
        let (width, height) = (self.width(), self.height());
        let [across, up] = match face {
            crate::Face::NegativeX | crate::Face::PositiveX => [2, 1],
            crate::Face::NegativeY | crate::Face::PositiveY => [0, 2],
            crate::Face::NegativeZ | crate::Face::PositiveZ => [0, 1],
        };
        (0..height).flat_map(move |v| {
            (0..width).map(move |u| {
                let mut block = origin;
                block[across] += u;
                block[up] += v;
                block
            })
        })
    }
}

/// Whether any of `cube_quads` or the `opaque` and `transparent` model draws carries a world
/// material tag.
pub(crate) fn tagged(
    cube_quads: &[crate::PackedQuad],
    opaque: &[crate::PackedModelDrawRef],
    transparent: &[crate::PackedModelDrawRef],
) -> bool {
    cube_quads.iter().any(|quad| quad.world_material() != 0)
        || opaque
            .iter()
            .chain(transparent)
            .any(|draw| draw.world_material() != 0)
}

impl crate::ChunkMesh {
    /// Whether any cube quad or model draw carries a world material tag, so the section draws
    /// in a world material pass.
    #[must_use]
    pub const fn has_world_materials(&self) -> bool {
        self.world_materials
    }
}
