//! Terrain observation limits shared by native and guest code.

/// Largest terrain query extent along each axis, in blocks.
pub const MAX_TERRAIN_EXTENT: u8 = 16;
/// Maximum cells in one terrain query.
pub const MAX_TERRAIN_CELLS: usize = (MAX_TERRAIN_EXTENT as usize).pow(3);
/// Maximum distinct terrain queries serviced for one mod per frame.
pub const MAX_TERRAIN_QUERIES: usize = 4;
/// Maximum terrain distance from the current player, along each axis.
pub const MAX_TERRAIN_DISTANCE: i64 = 128;
/// Cells re-sampled per frame across all mods; further changed queries wait a frame.
pub const MAX_TERRAIN_FRAME_CELLS: usize = 2 * MAX_TERRAIN_CELLS;
/// Distinct block-local boxes in one terrain region.
pub const MAX_TERRAIN_BOXES: usize = 4096;
/// Identifier, canonical-state and tag bytes in one terrain region.
pub const MAX_TERRAIN_METADATA_BYTES: usize = 256 * 1024;
/// Guest-memory bytes of one lowered terrain region.
pub const MAX_TERRAIN_RESPONSE_BYTES: usize = 512 * 1024;
