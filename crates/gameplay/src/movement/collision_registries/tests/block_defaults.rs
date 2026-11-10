//! Item effects resolve block names to their first palette permutation.
use std::sync::Arc;

use assets::NetworkIdMode::{Hashed, Sequential};

use super::{BREG_V2225, active_content_registry_protocol, bind, custom_block, synthetic_preg};

/// Vanilla names read the carrier's first permutation, and session names read this
/// session's first palette state until the next session replaces them.
#[test]
fn block_defaults_cover_vanilla_and_session_blocks_in_both_id_spaces() {
    let records =
        assets::read_registry_for_protocol(BREG_V2225, active_content_registry_protocol()).unwrap();
    let preg = synthetic_preg(active_content_registry_protocol(), BREG_V2225, &records);
    let mut registries = bind(BREG_V2225, &preg, active_content_registry_protocol()).unwrap();

    let wheat = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:wheat")
        .min_by_key(|record| record.sequential_id)
        .unwrap();
    assert_eq!(
        wheat.canonical_state.as_ref(),
        r#"{"growth":{"type":"int","value":0}}"#,
    );
    assert_eq!(
        registries.block_default_runtime_id(Sequential, "minecraft:wheat"),
        Some(wheat.sequential_id)
    );
    assert_eq!(
        registries.block_default_runtime_id(Hashed, "minecraft:wheat"),
        Some(wheat.network_hash)
    );
    assert_eq!(
        registries.block_default_runtime_id(Sequential, "test:crop"),
        None
    );

    let custom = protocol::CustomBlocks {
        blocks: vec![custom_block("test:other", 2), custom_block("test:crop", 8)].into(),
        vanilla_blocks: assets::server_defined_blocks()
            .iter()
            .map(|definition| Arc::from(definition.name))
            .collect(),
        skipped: 0,
    };
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    registries.begin_session_hashed_custom_blocks(&custom);
    assert_eq!(
        registries.block_default_runtime_id(Sequential, "test:crop"),
        Some(range.start + 2)
    );
    assert_eq!(
        registries.block_default_runtime_id(Hashed, "test:crop"),
        Some(custom.blocks[1].hashed_states()[0].hash)
    );

    let empty = protocol::CustomBlocks::default();
    registries.begin_session_custom_blocks(&empty).unwrap();
    registries.begin_session_hashed_custom_blocks(&empty);
    assert_eq!(
        registries.block_default_runtime_id(Sequential, "test:crop"),
        None
    );
    assert_eq!(
        registries.block_default_runtime_id(Hashed, "test:crop"),
        None
    );
}
