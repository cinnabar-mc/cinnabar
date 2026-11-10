use crate::{BlockEntityKey, BlockEntityNbt, ChunkStore, NbtCompound, NbtValue};

/// A piston arm actor reporting `state`, as a block-entity update delivers it.
fn arm(state: i8) -> BlockEntityNbt {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("PistonArm".into()));
    root.insert("State", NbtValue::Byte(state));
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

/// The arm's progress after `tick`.
fn progress(store: &ChunkStore, key: BlockEntityKey, tick: u64) -> f32 {
    store
        .block_entity(key)
        .unwrap()
        .piston_motion()
        .unwrap()
        .step_at(tick)
        .progress
}

#[test]
fn an_update_continues_from_the_progress_the_arm_reached() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 1, 2, 3);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store.set_simulation_tick(10).unwrap();
    store.commit_block_entity_update(key, arm(1)).unwrap();
    assert_eq!(progress(&store, key, 10), 0.0);
    assert_eq!(progress(&store, key, 11), 0.5);
    store.set_simulation_tick(11).unwrap();
    store.commit_block_entity_update(key, arm(3)).unwrap();
    assert_eq!(progress(&store, key, 11), 0.5);
    assert_eq!(progress(&store, key, 12), 0.0);
}

#[test]
fn moving_arms_revise_their_column_each_tick_until_they_settle() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 1, 2, 3);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store.set_simulation_tick(10).unwrap();
    store.commit_block_entity_update(key, arm(1)).unwrap();
    let mut revision = store.collision_revision(key.chunk());
    for tick in [11, 12] {
        let snapshot = store.collision_snapshot();
        store.set_simulation_tick(tick).unwrap();
        assert_ne!(
            store.collision_revision(key.chunk()),
            revision,
            "tick {tick}"
        );
        assert!(!std::sync::Arc::ptr_eq(
            &snapshot,
            &store.collision_snapshot()
        ));
        revision = store.collision_revision(key.chunk());
    }
    store.set_simulation_tick(13).unwrap();
    assert_eq!(store.collision_revision(key.chunk()), revision);
    assert!(store.moving_pistons.is_empty());
}

#[test]
fn a_restarted_clock_keeps_each_arm_where_it_was() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 1, 2, 3);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store.set_simulation_tick(100).unwrap();
    store.commit_block_entity_update(key, arm(1)).unwrap();
    store.set_simulation_tick(101).unwrap();
    let revision = store.collision_revision(key.chunk());
    store.set_simulation_tick(5).unwrap();
    assert_ne!(store.collision_revision(key.chunk()), revision);
    assert_eq!(progress(&store, key, 5), 0.5);
    assert_eq!(progress(&store, key, 6), 1.0);
}

#[test]
fn piston_actors_are_found_by_position_across_columns() {
    let mut store = ChunkStore::new();
    for (x, z) in [(15, 0), (16, 0), (16, 2), (40, 0)] {
        store
            .commit_block_entity_update(BlockEntityKey::new(0, x, 4, z), arm(2))
            .unwrap();
    }
    let found: Vec<_> = store
        .piston_actors_in(0, [15, 3, 0], [17, 5, 1])
        .map(|(key, _)| [key.x, key.z])
        .collect();
    assert_eq!(found, [[15, 0], [16, 0]]);
}
