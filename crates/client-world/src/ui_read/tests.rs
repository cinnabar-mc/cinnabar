use super::*;

/// Creates a world owner without scheduling workers or loading asset carriers.
fn world(dimension: i32) -> WorldAuthority {
    WorldAuthority::new(
        protocol::WorldBootstrap {
            local_player_unique_id: 11,
            local_player_runtime_id: 42,
            dimension,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

/// Encodes a small original custom title with the normal network NBT codec.
fn named_block(title: &str) -> world::BlockEntityNbt {
    let mut bytes = vec![10, 0, 8, 10];
    bytes.extend_from_slice(b"CustomName");
    bytes.push(u8::try_from(title.len()).unwrap());
    bytes.extend_from_slice(title.as_bytes());
    bytes.push(0);
    world::BlockEntityNbt::decode_prefix(&bytes).unwrap().0
}

#[test]
fn inventory_read_uses_global_negative_blocks_and_current_dimension() {
    let mut world = world(2);
    let position = BlockPos::from([-17, -65, -1]);
    for (dimension, title) in [(2, "Current"), (0, "Other")] {
        world
            .commit_block_entity_update(
                BlockEntityKey::new(dimension, position.x, position.y, position.z),
                named_block(title),
            )
            .unwrap();
    }
    let read = world.inventory_ui();
    let current = read.block_entity_compound(position).unwrap();
    assert_eq!(current.string("CustomName"), Some("Current"));
    assert!(
        read.block_entity_compound(BlockPos::new(-2, -65, -1))
            .is_none()
    );
    assert_eq!(
        read.block_entity_custom_name(position).as_deref(),
        Some("Current")
    );
    let (again, allocations) = test_alloc::count(|| read.block_entity_compound(position).unwrap());
    assert!(Arc::ptr_eq(&current, &again));
    assert_eq!(
        allocations, 0,
        "warm NBT observations share the owner's allocation"
    );

    world
        .commit_block_entity_update(BlockEntityKey::new(2, -17, -65, -1), named_block("Updated"))
        .unwrap();
    assert_eq!(
        world
            .inventory_ui()
            .block_entity_custom_name(position)
            .as_deref(),
        Some("Updated")
    );
    assert_eq!(current.string("CustomName"), Some("Current"));
}

#[test]
fn ui_reads_keep_owner_identity_without_allocating_or_spawning_a_local_actor() {
    let world = world(0);
    let other = self::world(0);
    let ((runtime, session, registry), allocations) = test_alloc::count(|| {
        let read = world.inventory_ui();
        let local = read.local_player();
        assert!(local.actor().is_none());
        assert!(local.profile().is_none());
        assert!(local.rig().is_none());
        assert!(local.pose().is_none());
        assert_eq!(world.nametag_ui().remote_actors().count(), 0);
        (
            local.runtime_id(),
            local.session_id(),
            read.item_presentation_generation(),
        )
    });
    assert_eq!(runtime, 42);
    assert_ne!(session, other.local_player_ui().session_id());
    assert!(Arc::ptr_eq(registry, world.item_presentation_generation()));
    assert!(!Arc::ptr_eq(registry, other.item_presentation_generation()));
    assert_eq!(allocations, 0);
}
