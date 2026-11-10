use super::*;
use client_shell::ui_resources::UiRuntimeResource;
use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
use render_scene::ModRenderScene;

/// Builds the overlay with the production frame ordering and no window or renderer.
fn overlay_app() -> App {
    let mut app = App::new();
    crate::game_frame::configure_client_frame_schedule(&mut app);
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Time<Real>>()
        .insert_resource(ClientWorld::default())
        .init_resource::<FatalError>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .insert_resource(UiPresentationResource(
            client_ui::test_support::mini_engine_presentation(),
        ))
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .init_resource::<ModRenderScene>();
    configure(&mut app);
    app
}

#[test]
fn debug_help_does_not_drop_the_held_stack() {
    for modifier_first in [false, true] {
        let mut app = overlay_app();
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        let entries = protocol::vanilla_item_registry();
        let network_id = entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:stone")
            .unwrap()
            .network_id;
        {
            let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
            runtime.publish_local_runtime_id(&mut player, 1, 1).unwrap();
            runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
            let ledger = runtime.inventory_ledger_mut(&mut player);
            ledger.apply_registry(&protocol::ItemRegistryEvent { entries });
            let mut slots = vec![
                NetworkItemStack::empty();
                inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT
            ];
            slots[0] = NetworkItemStack {
                network_id,
                count: 2,
                stack_network_id: 7,
                ..NetworkItemStack::empty()
            };
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
        }
        app.insert_resource(PlayerRuntime(player));
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.add_systems(
            Update,
            crate::ui_runtime::drive_world_inventory_keys.in_set(ClientFrameSet::UiAuthority),
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F3);
        if modifier_first {
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        app.update();
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(ledger.pending_request_count(), 0);
        assert_eq!(ledger.displayed_stack(0).unwrap().count, 2);
        assert!(
            app.world()
                .resource::<UiRuntimeResource>()
                .session
                .chat
                .messages()
                .iter()
                .any(|line| line.message.as_ref() == "[Debug] F3 + Q = Show this help list")
        );

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        app.update();
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(
            ledger.pending_request_count(),
            1,
            "a fresh Q still drops normally"
        );
        assert_eq!(ledger.displayed_stack(0).unwrap().count, 1);
    }
}

#[test]
fn debug_modifier_remapped_to_a_suffix_still_toggles_overlay() {
    use launcher::menu::settings_options::{DEBUG_OVERLAY_KEY, EXTRA_KEYS, KEY_BINDINGS};

    let row = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == DEBUG_OVERLAY_KEY)
            .unwrap();
    for key in [KeyCode::KeyG, KeyCode::KeyA, KeyCode::KeyD, KeyCode::KeyQ] {
        let mut app = overlay_app();
        let mut menu = client_shell::menu::MenuRuntime::new(false, 2, "Player".into());
        assert!(client_shell::menu::test_support::remap_control(
            &mut menu,
            row,
            semantic_input::PhysicalControl::KeyboardUsage(
                client_shell::physical_input::keyboard_usage(key).unwrap()
            ),
        ));
        app.insert_resource(menu);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        assert!(
            app.world().resource::<DebugOverlayState>().visible,
            "{key:?}"
        );
        assert!(
            !app.world()
                .resource::<DebugOverlayState>()
                .show_chunk_borders
        );
        assert!(
            app.world()
                .resource::<UiRuntimeResource>()
                .session
                .chat
                .messages()
                .is_empty()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(
            app.world().resource::<DebugOverlayState>().visible,
            "held {key:?}"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
        assert!(
            !app.world().resource::<DebugOverlayState>().visible,
            "second {key:?}"
        );
    }
}

/// Installs a session with an advertised range and enables borders at the supplied feet.
fn set_border_world(app: &mut App, dimension: i32, minimum_y: i32, maximum_y: i32, feet: Vec3) {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: feet.to_array(),
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            protocol::WorldEvent::DimensionHeights(vec![protocol::DimensionHeightDiagnostic {
                name: Arc::from(if dimension == 0 {
                    "minecraft:overworld"
                } else {
                    "example:raised"
                }),
                dimension: if dimension == 0 { 3 } else { dimension },
                minimum_y,
                maximum_y,
                generator: 1,
            }]),
        )
        .unwrap();
    app.world_mut().resource_mut::<ClientWorld>().stream = Some(stream);
    set_border_position(app, feet);
    app.world_mut()
        .resource_mut::<DebugOverlayState>()
        .show_chunk_borders = true;
}

/// Moves the diagnostic subject without requiring a physics simulation.
fn set_border_position(app: &mut App, feet: Vec3) {
    let mut view = LocalViewPose::default();
    view.set_subject_position(feet + Vec3::Y * protocol::STANDING_PLAYER_EYE_HEIGHT, feet);
    app.insert_resource(view);
}

#[test]
fn chunk_borders_reuse_the_buffer_inside_a_section_and_clear_when_hidden() {
    let mut app = overlay_app();
    set_border_world(&mut app, 0, 0, 256, Vec3::new(-0.5, 65.0, 1.0));
    app.update();
    let vertices = Arc::clone(&app.world().resource::<ModRenderScene>().debug_vertices);
    assert!(!vertices.is_empty());
    for feet in [Vec3::new(-0.5, 65.0, 1.0), Vec3::new(-15.0, 79.0, 15.0)] {
        set_border_position(&mut app, feet);
        app.world_mut().clear_trackers();
        app.update();
        let scene = app.world().get_resource_ref::<ModRenderScene>().unwrap();
        assert!(Arc::ptr_eq(&vertices, &scene.debug_vertices));
        assert!(!scene.is_changed());
    }
    app.world_mut()
        .resource_mut::<DebugOverlayState>()
        .show_chunk_borders = false;
    app.update();
    assert!(
        app.world()
            .resource::<ModRenderScene>()
            .debug_vertices
            .is_empty()
    );
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .get_resource_ref::<ModRenderScene>()
            .unwrap()
            .is_changed()
    );
    app.world_mut()
        .resource_mut::<DebugOverlayState>()
        .show_chunk_borders = true;
    app.update();
    assert!(Arc::ptr_eq(
        &vertices,
        &app.world().resource::<ModRenderScene>().debug_vertices
    ));
    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.update();
    assert!(
        app.world()
            .resource::<ModRenderScene>()
            .debug_vertices
            .is_empty()
    );
}

#[test]
fn chunk_borders_refresh_on_section_dimension_and_height_changes() {
    let mut app = overlay_app();
    set_border_world(&mut app, 0, 0, 256, Vec3::new(1.0, 65.0, 1.0));
    app.update();
    let mut previous = Arc::clone(&app.world().resource::<ModRenderScene>().debug_vertices);
    for feet in [
        Vec3::new(-0.5, 65.0, 1.0),
        Vec3::new(-0.5, 80.0, 1.0),
        Vec3::new(-0.5, 80.0, 16.0),
    ] {
        set_border_position(&mut app, feet);
        app.update();
        let next = Arc::clone(&app.world().resource::<ModRenderScene>().debug_vertices);
        assert!(!Arc::ptr_eq(&previous, &next));
        assert_ne!(previous.as_ref(), next.as_ref());
        previous = next;
    }
    for (dimension, roof) in [(1000, 256), (1000, 512)] {
        set_border_world(&mut app, dimension, 0, roof, Vec3::new(-0.5, 80.0, 16.0));
        app.update();
        let next = Arc::clone(&app.world().resource::<ModRenderScene>().debug_vertices);
        assert!(!Arc::ptr_eq(&previous, &next));
        previous = next;
    }
}

#[test]
fn chunk_borders_cover_the_active_dimensions_admitted_height_range() {
    for (dimension, minimum_y, maximum_y) in [(0, 0, 256), (1000, 256, 512)] {
        let mut app = overlay_app();
        set_border_world(
            &mut app,
            dimension,
            minimum_y,
            maximum_y,
            Vec3::new(1.0, minimum_y as f32 + 1.0, 1.0),
        );
        app.update();
        let scene = app.world().resource::<ModRenderScene>();
        let mut grid_heights: Vec<_> = scene
            .debug_vertices
            .iter()
            .filter(|vertex| vertex.color[0] == 1.0 && vertex.color[1] == 1.0)
            .map(|vertex| vertex.anchor[1])
            .collect();
        grid_heights.sort_by(f32::total_cmp);
        assert!((grid_heights.first().unwrap() - minimum_y as f32).abs() < 0.02);
        assert!((grid_heights.last().unwrap() - maximum_y as f32).abs() < 0.02);
        for section in 0..=(maximum_y - minimum_y) / world::SUB_CHUNK_SIDE as i32 {
            let y = minimum_y as f32 + (section as usize * world::SUB_CHUNK_SIDE) as f32;
            assert!(
                grid_heights.iter().any(|height| (height - y).abs() < 0.02),
                "missing grid at {y}"
            );
        }
    }
}
