//! The block and entity queries the simulation makes against the caller's world.

use crate::block::Block;
use crate::math::Aabb;

/// Lowest block Y outside the 1.7.10 world column.
pub const WORLD_HEIGHT: i32 = 256;

/// A read-only view of a Java Edition 1.7.10 world.
///
/// Implementations return [`Block::AIR`] for positions outside the build height or in columns
/// that are not loaded, as the game's world does.
pub trait World {
    /// The block at a position, or air when the position is not available.
    fn block(&self, x: i32, y: i32, z: i32) -> Block;

    /// Whether the chunk column at chunk coordinates `(chunk_x, chunk_z)` is loaded.
    fn chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// Whether a block position lies inside the build height of a loaded column.
    fn block_exists(&self, x: i32, y: i32, z: i32) -> bool {
        (0..WORLD_HEIGHT).contains(&y) && self.chunk_loaded(x >> 4, z >> 4)
    }

    /// Whether every column touched by the block range is loaded and the range reaches the
    /// build height.
    fn chunks_exist(&self, min: [i32; 3], max: [i32; 3]) -> bool {
        if max[1] < 0 || min[1] >= WORLD_HEIGHT {
            return false;
        }
        ((min[0] >> 4)..=(max[0] >> 4)).all(|chunk_x| {
            ((min[2] >> 4)..=(max[2] >> 4)).all(|chunk_z| self.chunk_loaded(chunk_x, chunk_z))
        })
    }

    /// Appends the solid boxes of entities that block movement, such as boats, near `query`.
    /// The simulation keeps those that intersect it.
    fn entity_boxes(&self, _query: &Aabb, _out: &mut Vec<Aabb>) {}
}
