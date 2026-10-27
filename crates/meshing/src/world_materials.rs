//! Blocks a programmable world material shades, as terrain meshing reads them.

/// One block network value a programmable world material shades.
///
/// Meshing takes these sorted by [`Self::network_value`], one per value. A bound block's cube
/// faces carry its tag, stay one block wide (a world material may move each corner) and draw
/// only in their material's pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldMaterialBlock {
    /// The block's network value.
    pub network_value: u32,
    /// One more than the material slot, at most [`crate::PackedQuad::MAX_WORLD_MATERIAL`].
    pub tag: u8,
    /// Whether the material lets what lies behind the block show (cutout or blended). Such a
    /// block hides no face of a block with another tag or none, as glass hides no stone face,
    /// whatever its Bedrock look; touching faces of blocks with the same tag stay hidden, as
    /// glass hides glass.
    pub see_through: bool,
}

impl WorldMaterialBlock {
    /// A block bound to `tag` whose material lets the world behind it show when `see_through`.
    #[must_use]
    pub const fn new(network_value: u32, tag: u8, see_through: bool) -> Self {
        Self {
            network_value,
            tag,
            see_through,
        }
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

/// Whether `blocks` make `network_value` see-through, so it hides no other block's faces.
pub(crate) fn see_through(blocks: &[WorldMaterialBlock], network_value: u32) -> bool {
    find(blocks, network_value).is_some_and(|block| block.see_through)
}

/// Whether a face of block `cell` is hidden by block `neighbour` because both carry the same
/// world material tag; untagged blocks never match.
pub(crate) fn same_tag(blocks: &[WorldMaterialBlock], cell: u32, neighbour: u32) -> bool {
    let cell = tag(blocks, cell);
    cell != 0 && cell == tag(blocks, neighbour)
}
