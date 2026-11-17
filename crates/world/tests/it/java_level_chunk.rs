//! The Go core translates Java Edition 1.7.10 columns to LevelChunk payloads.
//!
//! `java_level_chunk.bin` is its payload for a scripted column and `java_level_chunk.json`
//! names blocks that column holds; `core/javaedition/translate` regenerates and checks both.
//! The client's chunk decoder must read the payload to its end and find those blocks.

use serde::Deserialize;
use world::{
    ChunkKey, ChunkStore, DecodedLevelChunk, DimensionSlots, RawBiomeIds, RawBlockIds, SubChunkKey,
};

const PAYLOAD: &[u8] = include_bytes!("../fixtures/java_level_chunk.bin");
const EXPECTED: &str = include_str!("../fixtures/java_level_chunk.json");

/// The translator addresses blocks by state hash, so the decoder keeps every id.
const IDS: RawBlockIds = RawBlockIds { air: 0 };
const BIOMES: RawBiomeIds = RawBiomeIds { default_biome: 0 };
/// The Bedrock overworld: 24 sub-chunks from Y -64.
const OVERWORLD: DimensionSlots = DimensionSlots {
    base_sub_chunk_y: -4,
    count: 24,
};

#[derive(Deserialize)]
struct Expected {
    sub_chunk_count: usize,
    blocks: Vec<Cell>,
    biome_at_origin: u32,
    biome_elsewhere: u32,
}

#[derive(Deserialize)]
struct Cell {
    x: u8,
    y: i32,
    z: u8,
    network_id: u32,
    state: String,
}

#[test]
fn translated_java_column_decodes_to_its_blocks_and_biomes() {
    let expected: Expected = serde_json::from_str(EXPECTED).expect("expected blocks");
    let chunk = ChunkKey::new(0, 0, 0);
    let decoded = DecodedLevelChunk::decode_inline(
        chunk,
        OVERWORLD,
        expected.sub_chunk_count,
        PAYLOAD,
        &IDS,
        &BIOMES,
    );
    let mut store = ChunkStore::new();
    let applied = store
        .commit_level_chunk(chunk, decoded)
        .expect("the column commits");
    assert_eq!(
        applied.bytes_consumed,
        PAYLOAD.len(),
        "the decoder reads the whole payload"
    );

    assert!(!expected.blocks.is_empty());
    for cell in &expected.blocks {
        let key = SubChunkKey::from_chunk(chunk, cell.y.div_euclid(16));
        let local_y = cell.y.rem_euclid(16) as u8;
        let found = store
            .sub_chunk(key)
            .and_then(|sub_chunk| sub_chunk.runtime_id(0, cell.x, local_y, cell.z));
        // Sections the server did not send hold no storage: the client reads them as air.
        let is_air = cell.state.starts_with("minecraft:air[");
        match found {
            Some(id) => assert_eq!(
                id, cell.network_id,
                "{} at {},{},{}",
                cell.state, cell.x, cell.y, cell.z
            ),
            None => assert!(
                is_air,
                "{} at {},{},{} is missing",
                cell.state, cell.x, cell.y, cell.z
            ),
        }
    }
    // Java biomes do not vary with height: every sub-chunk repeats the column's.
    for sub_chunk_y in [-4, 0, 19] {
        let key = SubChunkKey::from_chunk(chunk, sub_chunk_y);
        assert_eq!(store.biome_id(key, 0, 5, 0), Some(expected.biome_at_origin));
        assert_eq!(store.biome_id(key, 9, 5, 3), Some(expected.biome_elsewhere));
    }
}
