use std::sync::Arc;

use super::ChunkStore;
use crate::{
    BlockEntityKey, BlockEntityNbt, BlockUpdate, ChunkKey, DecodedBiomeColumn, NbtCompound,
    NbtValue, RawBiomeIds, SubChunkKey,
};

/// Encodes a sign at the origin through the public NBT decoder.
fn sign(text: &str) -> BlockEntityNbt {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("Sign".into()));
    for key in ["x", "y", "z"] {
        root.insert(key, NbtValue::Int(0));
    }
    root.insert("Text", NbtValue::String(text.into()));
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

/// Whether `change` retired the store's content revision.
fn changes(store: &mut ChunkStore, change: impl FnOnce(&mut ChunkStore)) -> bool {
    let before = store.content_revision();
    change(store);
    store.content_revision() != before
}

#[test]
fn every_visible_change_retires_the_revision_and_no_op_commits_keep_it() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let entity = BlockEntityKey::new(0, 0, 0, 0);
    assert_ne!(
        store.content_revision(),
        ChunkStore::new().content_revision()
    );

    assert!(changes(&mut store, |store| {
        store.mark_sub_chunk_loaded(key).unwrap();
    }));
    assert!(!changes(&mut store, |store| {
        store.mark_sub_chunk_loaded(key).unwrap();
    }));
    assert!(changes(&mut store, |store| {
        store.mark_chunk_loaded(key.chunk()).unwrap();
    }));
    assert!(!changes(&mut store, |store| {
        store.mark_chunk_loaded(key.chunk()).unwrap();
    }));

    let place = BlockUpdate::new(1, 2, 3, 0, 7);
    assert!(changes(&mut store, |store| {
        store.update_block(key, place, 0).unwrap();
    }));
    assert!(!changes(&mut store, |store| {
        store.update_block(key, place, 0).unwrap();
    }));
    assert!(changes(&mut store, |store| {
        store
            .update_block(key, BlockUpdate::new(1, 2, 3, 0, 0), 0)
            .unwrap();
    }));

    assert!(changes(&mut store, |store| {
        store.commit_block_entity_update(entity, sign("a")).unwrap();
    }));
    assert!(!changes(&mut store, |store| {
        store.commit_block_entity_update(entity, sign("a")).unwrap();
    }));
    assert!(changes(&mut store, |store| {
        store.commit_block_entity_update(entity, sign("b")).unwrap();
    }));

    assert!(changes(&mut store, |store| {
        store.commit_biome_column(
            key.chunk(),
            DecodedBiomeColumn::decode(0, 1, &[1, 84], &RawBiomeIds { default_biome: 0 }),
        );
    }));
    assert!(changes(&mut store, |store| {
        store.evict_chunk(key.chunk());
    }));
    assert!(!changes(&mut store, |store| {
        store.evict_chunk(ChunkKey::new(0, 5, 5));
    }));
}

#[test]
fn snapshots_never_share_a_revision_with_their_source() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 1), 0)
        .unwrap();
    let snapshot = store.collision_snapshot();
    assert_ne!(snapshot.content_revision(), store.content_revision());
    assert_eq!(
        snapshot.content_revision(),
        store.collision_snapshot().content_revision()
    );
}

#[test]
fn held_indexes_match_shared_identity_until_an_edit() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let entity = BlockEntityKey::new(0, 0, 0, 0);
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 1), 0)
        .unwrap();
    store.commit_block_entity_update(entity, sign("a")).unwrap();
    let chunk = store.chunk(key.chunk()).unwrap();
    let sections = Arc::downgrade(&chunk.shared_sub_chunks());
    let entities = Arc::downgrade(&chunk.shared_block_entities());
    assert!(chunk.holds_sub_chunks(&sections));
    assert!(chunk.holds_block_entities(&entities));
    assert!(!chunk.holds_sub_chunks(&std::sync::Weak::new()));

    store
        .update_block(key, BlockUpdate::new(1, 0, 0, 0, 1), 0)
        .unwrap();
    let chunk = store.chunk(key.chunk()).unwrap();
    assert!(!chunk.holds_sub_chunks(&sections));
    assert!(chunk.holds_block_entities(&entities));

    store.commit_block_entity_update(entity, sign("b")).unwrap();
    let chunk = store.chunk(key.chunk()).unwrap();
    assert!(!chunk.holds_block_entities(&entities));
    // The reference-counted check agrees with the pointer check after each edit.
    assert!(
        !sections
            .upgrade()
            .is_some_and(|held| Arc::ptr_eq(&held, &chunk.shared_sub_chunks()))
    );
}
