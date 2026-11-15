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

    /// Whether the server has sent the chunk column at chunk coordinates `(chunk_x, chunk_z)`:
    /// the game's `Chunk.isChunkLoaded`, which is false for the empty chunk that stands in for
    /// a column never sent. Only gravity reads it.
    fn chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// Whether a block position lies inside the build height. The client's chunk provider
    /// reports every column as existing (`ChunkProviderClient.chunkExists`), so this
    /// `World.blockExists` checks only the height.
    fn block_exists(&self, _x: i32, y: i32, _z: i32) -> bool {
        (0..WORLD_HEIGHT).contains(&y)
    }

    /// Whether the block range reaches the build height: `World.checkChunksExist`, which on
    /// the client finds every column existing, as [`World::block_exists`] does.
    fn chunks_exist(&self, min: [i32; 3], max: [i32; 3]) -> bool {
        max[1] >= 0 && min[1] < WORLD_HEIGHT
    }

    /// Appends the solid boxes of entities that block movement, such as boats, near `query`.
    /// The simulation keeps those that intersect it.
    fn entity_boxes(&self, _query: &Aabb, _out: &mut Vec<Aabb>) {}
}
