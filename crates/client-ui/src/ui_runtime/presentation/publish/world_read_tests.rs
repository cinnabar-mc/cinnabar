use super::*;
use world::{BlockEntityKey, BlockEntityNbt, NbtCompound, NbtValue};

/// Creates an admitted world owner with no streaming workers.
fn world() -> client_world::WorldAuthority {
    client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            local_player_unique_id: 11,
            local_player_runtime_id: 42,
            dimension: 0,
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

/// Admits a station window at a negative global block position before frame capture.
fn open(window_type: i8) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            protocol::InventoryEvent::Open(protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(7),
                window_type,
                position: [-17, 64, -1],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    assert!(runtime.inventory_open());
    (player, runtime)
}

/// Commits an original typed NBT fixture at the open station's position.
fn commit(world: &mut client_world::WorldAuthority, root: &NbtCompound) {
    let bytes = root.encode_root().unwrap();
    let nbt = BlockEntityNbt::decode_prefix(&bytes).unwrap().0;
    world
        .commit_block_entity_update(BlockEntityKey::new(0, -17, 64, -1), nbt)
        .unwrap();
}

#[test]
fn inventory_world_read_opens_lectern_before_same_frame_screen_and_title_capture() {
    let mut world = world();
    let mut page = NbtCompound::default();
    page.insert("text", NbtValue::String("Original page".into()));
    let mut tag = NbtCompound::default();
    tag.insert("pages", NbtValue::List(vec![NbtValue::Compound(page)]));
    let mut book = NbtCompound::default();
    book.insert("tag", NbtValue::Compound(tag));
    let mut root = NbtCompound::default();
    root.insert("book", NbtValue::Compound(book));
    root.insert("CustomName", NbtValue::String("Current title".into()));
    root.insert("page", NbtValue::Int(99));
    commit(&mut world, &root);
    let (player, mut runtime) = open(protocol::WINDOW_TYPE_LECTERN);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(world.inventory_ui()),
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        Default::default(),
    );
    let state = runtime
        .screen_state()
        .book
        .as_ref()
        .expect("book opens during capture");
    assert_eq!(state.pages, ["Original page"]);
    assert_eq!(
        state.page, 0,
        "the admitted page clamps to the observed book"
    );
    assert_eq!(
        inventory_pointer::InventoryScreen::of_runtime(&player, &runtime),
        inventory_pointer::InventoryScreen::Book
    );
    assert_eq!(
        presentation.hud_frame().window_text.custom_title.as_deref(),
        Some("Current title")
    );
}

#[test]
fn inventory_world_read_keeps_storage_cache_identity_and_retires_it_with_session() {
    let mut world = world();
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("Barrel".into()));
    commit(&mut world, &root);
    let (mut player, mut runtime) = open(protocol::WINDOW_TYPE_CONTAINER);
    observe_storage_block(&mut runtime, &player, Some(world.inventory_ui()));
    let (position, identity) = runtime.session.storage_block.as_ref().unwrap().clone();
    assert_eq!(position, [-17, 64, -1]);
    observe_storage_block(&mut runtime, &player, Some(world.inventory_ui()));
    assert!(Arc::ptr_eq(
        &identity,
        &runtime.session.storage_block.as_ref().unwrap().1
    ));
    player.begin_session(2);
    runtime.begin_session(2);
    assert!(runtime.session.storage_block.is_none());
}
