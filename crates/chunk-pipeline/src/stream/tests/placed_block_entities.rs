use super::*;

const POSITION: [i32; 3] = [1, -63, 2];

/// A stream holding one loaded stone section under [`POSITION`].
fn loaded_stone_stream() -> (WorldStream, u64) {
    let mut stream = block_entity_visual_stream();
    stream
        .submit(
            1,
            inline_block_entity_event(0, block_entity_runtime_id("minecraft:stone"), Vec::new()),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    (stream, 2)
}

/// Sets the block at [`POSITION`] with one server block update.
fn set_block(stream: &mut WorldStream, sequence: &mut u64, name: &str) {
    stream
        .submit(
            *sequence,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: POSITION,
                layer: 0,
                network_id: block_entity_runtime_id(name),
            }]),
        )
        .unwrap();
    *sequence += 1;
    complete_pending_decode_jobs(stream);
}

/// The block entity retained at [`POSITION`].
fn stored(stream: &WorldStream) -> Option<Arc<world::BlockEntityNbt>> {
    stream.authority.terrain().block_entity(BlockEntityKey::new(
        0,
        POSITION[0],
        POSITION[1],
        POSITION[2],
    ))
}

#[test]
fn placed_block_entity_blocks_get_an_entity_without_server_data() {
    for (name, id) in [
        ("minecraft:chest", "Chest"),
        ("minecraft:trapped_chest", "Chest"),
        ("minecraft:copper_chest", "Chest"),
        ("minecraft:ender_chest", "EnderChest"),
        ("minecraft:red_shulker_box", "ShulkerBox"),
        ("minecraft:frame", "ItemFrame"),
        ("minecraft:glow_frame", "GlowItemFrame"),
        ("minecraft:decorated_pot", "DecoratedPot"),
    ] {
        let (mut stream, mut sequence) = loaded_stone_stream();
        set_block(&mut stream, &mut sequence, name);
        let entity = stored(&stream).unwrap_or_else(|| panic!("{name} has no block entity"));
        assert_eq!(entity.id(), Some(id), "{name}");
        assert_eq!(entity.embedded_position(), Some(POSITION), "{name}");

        set_block(&mut stream, &mut sequence, "minecraft:air");
        assert!(
            stored(&stream).is_none(),
            "{name} keeps its entity after breaking"
        );
    }
}

#[test]
fn block_entity_data_before_its_block_survives_and_a_replaced_kind_does_not() {
    let (mut stream, mut sequence) = loaded_stone_stream();
    let data = block_entity_nbt_with_marker("Chest", POSITION, 7);
    stream
        .submit(
            sequence,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: POSITION,
                nbt: data.clone(),
            }),
        )
        .unwrap();
    sequence += 1;
    complete_pending_decode_jobs(&mut stream);
    set_block(&mut stream, &mut sequence, "minecraft:chest");
    assert_eq!(stored(&stream).unwrap().bytes(), &data[..]);

    // Changing between blocks of one entity kind keeps the entity.
    set_block(&mut stream, &mut sequence, "minecraft:trapped_chest");
    assert_eq!(stored(&stream).unwrap().bytes(), &data[..]);

    set_block(&mut stream, &mut sequence, "minecraft:ender_chest");
    let replaced = stored(&stream).unwrap();
    assert_eq!(replaced.id(), Some("EnderChest"));
    assert_ne!(replaced.bytes(), &data[..]);
}
