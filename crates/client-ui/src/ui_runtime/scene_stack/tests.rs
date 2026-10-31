use std::sync::Arc;

use serde_json::json;

use {super::*, launcher::menu::MenuScreen};

fn settings(value: serde_json::Value) -> ScreenSettings {
    let serde_json::Value::Object(map) = value else {
        panic!("object");
    };
    ScreenSettings::from_properties(&map.into_iter().collect())
}

/// The vanilla HUD's declared settings; every other screen takes the defaults.
fn vanilla_like_table() -> Arc<ScreenSettingsTable> {
    let hud = settings(json!({"is_showing_menu": false, "should_steal_mouse": true,
        "low_frequency_rendering": true, "absorbs_input": false}));
    let chest = settings(json!({"close_on_player_hurt": true}));
    Arc::new(ScreenSettingsTable(HashMap::from([
        (json_ui::vanilla::Reference::HudHudScreen.as_str(), hud),
        ("crafting.inventory_screen", chest),
    ])))
}

fn in_world(
    player_runtime: &mut player_state::PlayerState,
    table: Arc<ScreenSettingsTable>,
) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    runtime.publish_inventory_authority(player_runtime, protocol::InventoryAuthority::Server);
    runtime.set_screen_settings(table);
    runtime
}

fn host(menu: Option<MenuScreen>) -> SceneHost {
    SceneHost {
        menu,
        over_world: true,
        loading: None,
    }
}

#[test]
fn death_overlay_keeps_the_hud_visible_and_absorbs_gameplay_input() {
    let mut player = player_state::PlayerState::new(1);
    let table = Arc::new(ScreenSettingsTable(HashMap::from([
        (
            json_ui::vanilla::Reference::HudHudScreen.as_str(),
            settings(
                json!({"absorbs_input":false,"is_showing_menu":false,"render_only_when_topmost":true}),
            ),
        ),
        (
            "death.death_screen",
            settings(json!({"render_game_behind":false})),
        ),
    ])));
    let runtime = in_world(&mut player, table);
    let stack = runtime.scenes(&player, host(Some(MenuScreen::Death)));
    assert_eq!(
        stack.visible(false),
        [Scene::Gameplay, Scene::Hud, Scene::Menu(MenuScreen::Death)]
    );
    assert!(stack.top().unwrap().settings.absorbs_input);
    assert!(!stack.top().unwrap().settings.should_steal_mouse);
}

#[test]
fn credits_own_gameplay_input_until_the_session_bound_completion_is_sent() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = in_world(&mut player, vanilla_like_table());
    assert!(runtime.credits_mut().open(42, 8, 0));
    assert!(!runtime.gameplay_input(&player, None));
    assert!(!runtime.steals_mouse(&player, None));
    assert_eq!(
        runtime
            .scenes(&player, host(Some(MenuScreen::Pause)))
            .top()
            .unwrap()
            .key,
        Scene::Credits
    );
    runtime.credits_mut().skip(10);
    assert_eq!(
        runtime
            .credits_mut()
            .flush(Some(42), |_| Err(super::super::FormTransportError::Full)),
        Err(super::super::FormTransportError::Full)
    );
    assert!(!runtime.gameplay_input(&player, None));
    runtime.credits_mut().flush(Some(42), |_| Ok(())).unwrap();
    assert!(runtime.gameplay_input(&player, None));
}

// Only the HUD over the world: clicks reach gameplay and the mouse is captured.
#[test]
fn hud_only_routes_clicks_to_gameplay() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let runtime = in_world(&mut player_runtime, vanilla_like_table());
    assert!(runtime.gameplay_input(&player_runtime, None));
    assert!(!runtime.ui_focused(&player_runtime));
    assert!(runtime.steals_mouse(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Hud]
    );
    let crosshair = settings(
        json!({"render_only_when_topmost": false, "absorbs_input": false,
        "is_showing_menu": false, "should_steal_mouse": false}),
    );
    let mut table = (*vanilla_like_table()).clone();
    table.0.insert(
        json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen.as_str(),
        crosshair,
    );
    let mut runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(
        runtime.steals_mouse(&player_runtime, None),
        "the crosshair overlay sits under the HUD"
    );
    runtime.open_chat(&mut player_runtime);
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Crosshair, Scene::Chat]
    );
}

// An absorbing screen (inventory, chat, pause) takes clicks and frees the mouse.
#[test]
fn absorbing_screens_route_clicks_to_the_ui() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    runtime.toggle_inventory(&mut player_runtime, None);
    assert!(runtime.inventory_open());
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert!(!runtime.steals_mouse(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).visible(false),
        [Scene::Gameplay, Scene::Container]
    );
    runtime.close_inventory(&mut player_runtime);
    runtime.open_chat(&mut player_runtime);
    assert!(!runtime.gameplay_input(&player_runtime, None));
    runtime.close_chat();
    assert!(runtime.gameplay_input(&player_runtime, None));

    let paused = runtime.scenes(&player_runtime, host(Some(MenuScreen::Pause)));
    assert!(!paused.receives_input(Scene::Gameplay));
    assert!(!paused.steals_mouse());
    assert_eq!(
        paused.visible(false),
        [Scene::Gameplay, Scene::Menu(MenuScreen::Pause)]
    );
}

#[test]
fn resource_pack_reload_cover_blocks_gameplay_and_menu_input() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    runtime.set_resource_pack_loading(true);

    let stack = runtime.scenes(&player_runtime, host(Some(MenuScreen::Pause)));
    assert_eq!(
        stack.top().map(|scene| scene.key),
        Some(Scene::ResourcePackLoading)
    );
    assert_eq!(stack.input_targets(), [Scene::ResourcePackLoading]);
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert!(!runtime.steals_mouse(&player_runtime, None));

    let launcher = SceneHost {
        menu: Some(MenuScreen::Settings),
        over_world: false,
        loading: None,
    };
    assert_eq!(
        runtime
            .scenes(&player_runtime, launcher)
            .top()
            .map(|scene| scene.key),
        Some(Scene::ResourcePackLoading)
    );

    // World entry keeps the vanilla loading scene in charge; the pack cover never replaces it.
    let world_entry = SceneHost {
        menu: None,
        over_world: false,
        loading: Some(ui::LoadingStage::BuildingTerrain),
    };
    let scenes = runtime.scenes(&player_runtime, world_entry);
    assert_eq!(scenes.input_targets(), [Scene::Loading]);
    assert!(!scenes.contains(Scene::ResourcePackLoading));
}

// A pack that makes the HUD absorb input takes clicks from gameplay, as vanilla would.
#[test]
fn pack_declared_absorbing_hud_blocks_gameplay() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let hud = settings(json!({"is_showing_menu": false, "should_steal_mouse": true}));
    let table = ScreenSettingsTable(HashMap::from([(
        json_ui::vanilla::Reference::HudHudScreen.as_str(),
        hud,
    )]));
    let runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(!runtime.gameplay_input(&player_runtime, None));
}

// The launcher's menus have no world under them; its start screen takes input.
#[test]
fn launcher_menus_have_no_world_beneath() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let runtime = in_world(&mut player_runtime, vanilla_like_table());
    let launcher = SceneHost {
        over_world: false,
        ..host(Some(MenuScreen::Home))
    };
    let stack = runtime.scenes(&player_runtime, launcher);
    assert!(!stack.contains(Scene::Gameplay));
    assert_eq!(stack.visible(false), [Scene::Menu(MenuScreen::Home)]);
    assert!(!runtime.scenes(&player_runtime, launcher).steals_mouse());
}

// A health drop closes a top container whose screen asks for it.
#[test]
fn hurt_closes_a_top_container_that_asks() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    runtime.toggle_inventory(&mut player_runtime, None);
    runtime.note_player_hurt();
    runtime.close_scenes_on_player_hurt(&mut player_runtime, None);
    assert!(!runtime.inventory_open());
}

// Being hit leaves chat open even though the chat screen asks to close when hurt.
#[test]
fn hurt_keeps_chat_open() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut table = vanilla_like_table().as_ref().0.clone();
    table.insert(
        ui_screens::chat_screen::CHAT_SCREEN,
        settings(json!({"close_on_player_hurt": true})),
    );
    let mut runtime = in_world(&mut player_runtime, Arc::new(ScreenSettingsTable(table)));
    runtime.open_chat(&mut player_runtime);
    runtime.note_player_hurt();
    runtime.close_scenes_on_player_hurt(&mut player_runtime, None);
    assert!(runtime.chat_focused());
}

// The real carrier's HUD passes input through and a container absorbs it.
#[test]
fn carrier_settings_route_input_like_vanilla() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(carrier) = ui_engine::test_support::carrier() else {
        eprintln!(
            "skipping carrier_settings_route_input_like_vanilla: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let catalog = Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let table = ScreenSettingsTable::for_catalog(&catalog, &Context::retail(true));
    let hud = table
        .get(json_ui::vanilla::Reference::HudHudScreen.as_str())
        .expect("the HUD screen resolves");
    assert!(!hud.absorbs_input && hud.should_steal_mouse);
    let mut runtime = in_world(&mut player_runtime, Arc::new(table));
    assert!(runtime.gameplay_input(&player_runtime, None));
    runtime.toggle_inventory(&mut player_runtime, None);
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).closes_on_hurt(),
        None
    );
}

// A pushed scene enters with a push fade; the one it covered re-enters with a pop fade.
#[test]
fn scene_clocks_time_entrance_transitions() {
    let mut clocks = SceneClocks::default();
    clocks.observe(&[], 0.0);
    clocks.observe(&[Scene::Gameplay, Scene::Hud], 1.0);
    assert_eq!(
        clocks.clocks(Scene::Hud).get("screen.entrance_push"),
        Some(&1.0)
    );
    clocks.observe(
        &[Scene::Gameplay, Scene::Hud, Scene::Menu(MenuScreen::Pause)],
        2.0,
    );
    let pause = clocks.clocks(Scene::Menu(MenuScreen::Pause));
    assert_eq!(pause.get("screen.entrance_push"), Some(&2.0));
    clocks.observe(&[Scene::Gameplay, Scene::Hud], 3.0);
    assert_eq!(
        clocks.clocks(Scene::Hud).get("screen.entrance_pop"),
        Some(&3.0)
    );
    assert!(clocks.clocks(Scene::Menu(MenuScreen::Pause)).is_empty());
}

// Switching between tabs of one vanilla screen is not a new entrance.
#[test]
fn menu_tabs_share_one_entrance() {
    let mut clocks = SceneClocks::default();
    clocks.observe(&[], 0.0);
    clocks.observe(&[Scene::Menu(MenuScreen::Play)], 1.0);
    clocks.observe(&[Scene::Menu(MenuScreen::Servers)], 2.0);
    let servers = clocks.clocks(Scene::Menu(MenuScreen::Servers));
    assert_eq!(servers.get("screen.entrance_push"), Some(&1.0));
}

#[test]
fn launcher_scene_policy_ignores_json_ui_menu_roots() {
    let mut player = player_state::PlayerState::new(1);
    let mut hostile = ScreenSettingsTable::default();
    hostile.0.insert(
        launcher::menu::menu_reference(MenuScreen::Home).unwrap(),
        ScreenSettings {
            should_steal_mouse: true,
            always_accepts_input: true,
            render_game_behind: true,
            ..Default::default()
        },
    );
    let runtime = in_world(&mut player, Arc::new(hostile));
    let launcher = SceneHost {
        menu: Some(MenuScreen::Home),
        over_world: false,
        loading: None,
    };
    assert!(!runtime.scenes(&player, launcher).steals_mouse());
    assert!(!runtime.scenes(&player, launcher).contains(Scene::Gameplay));
}

// A server pack's progress screen cannot change input or cursor capture during client-owned
// world-entry loading; the in-session dimension change still follows it.
#[test]
fn world_entry_loading_ignores_the_server_progress_screen_policy() {
    let mut player = player_state::PlayerState::new(1);
    let hostile = Arc::new(ScreenSettingsTable(HashMap::from([(
        ui_screens::loading_screen::LOADING_SCREEN,
        settings(json!({"absorbs_input": false, "should_steal_mouse": true})),
    )])));
    let runtime = in_world(&mut player, hostile);
    let loading = |stage| SceneHost {
        loading: Some(stage),
        ..host(None)
    };
    for stage in [
        ui::LoadingStage::Connecting,
        ui::LoadingStage::BuildingTerrain,
    ] {
        let scenes = runtime.scenes(&player, loading(stage));
        assert!(!scenes.steals_mouse(), "{stage:?}");
        assert_eq!(scenes.input_targets(), [Scene::Loading], "{stage:?}");
    }
    let dimension = runtime.scenes(&player, loading(ui::LoadingStage::ChangingDimension));
    assert!(dimension.steals_mouse());
    assert!(dimension.input_targets().contains(&Scene::Gameplay));
}

/// Requests a server form and reports whether it was admitted rather than answered busy.
fn form_admitted(runtime: &mut UiRuntime, player_runtime: &mut player_state::PlayerState) -> bool {
    runtime
        .apply(
            player_runtime,
            crate::ui_runtime::SequencedUiEvent {
                session_id: runtime.session_id(),
                fifo_sequence: 1,
                local_millis: 1,
                server_tick: None,
                event: protocol::UiEvent::Form(protocol::FormRequestEvent {
                    form_id: 3,
                    kind: protocol::FormKind::Menu,
                    title: None,
                    json: "{}".into(),
                    model: protocol::ServerFormModel::Unsupported(
                        protocol::UnsupportedForm::Family,
                    ),
                }),
            },
        )
        .unwrap();
    let admitted = runtime.server_forms().active().is_some();
    assert_eq!(
        runtime.server_forms().queued_busy_count(),
        usize::from(!admitted)
    );
    admitted
}

/// A server menu built from a container answers a form only when the container is a menu.
#[test]
fn forms_open_over_containers_that_are_not_menus() {
    for (showing_menu, admitted) in [(None, false), (Some(true), false), (Some(false), true)] {
        let mut player_runtime = player_state::PlayerState::new(1);
        let mut table = HashMap::new();
        if let Some(flag) = showing_menu {
            table.insert(
                "crafting.inventory_screen",
                settings(json!({ "is_showing_menu": flag })),
            );
        }
        let mut runtime = in_world(&mut player_runtime, Arc::new(ScreenSettingsTable(table)));
        runtime.toggle_inventory(&mut player_runtime, None);
        assert!(runtime.inventory_open());
        assert_eq!(
            form_admitted(&mut runtime, &mut player_runtime),
            admitted,
            "is_showing_menu {showing_menu:?}"
        );
    }
}

/// Admits a generic storage window before any world observation refreshes its block identity.
fn open_storage_window(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    position: [i32; 3],
) {
    let session = runtime.session_id();
    runtime.publish_inventory_authority(player, protocol::InventoryAuthority::Server);
    for (sequence, event) in [
        protocol::InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: protocol::ContainerIdentity::window(7),
            window_type: protocol::WINDOW_TYPE_CONTAINER,
            position,
            runtime_entity_id: -1,
        }),
        protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity {
                slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                ..protocol::ContainerIdentity::window(7)
            },
            slots: vec![protocol::NetworkItemStack::empty(); 27].into(),
            storage_item: protocol::NetworkItemStack::empty(),
        }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .enqueue_inventory_event(player, session, sequence as u64 + 1, event)
            .unwrap();
    }
    runtime.drain_pending_inventory(player);
    assert!(runtime.inventory_open());
}

#[test]
fn session_storage_observation_cannot_select_the_next_servers_screen_before_capture() {
    use json_ui::vanilla::Reference;
    let mut player = player_state::PlayerState::new(1);
    let table = Arc::new(ScreenSettingsTable(HashMap::from([
        (
            Reference::ChestSmallChestScreen.as_str(),
            settings(json!({"is_showing_menu": true})),
        ),
        (
            Reference::ChestBarrelScreen.as_str(),
            settings(json!({"is_showing_menu": false})),
        ),
    ])));
    let mut runtime = in_world(&mut player, table);
    let position = [3, 64, -2];
    open_storage_window(&mut runtime, &mut player, position);
    runtime.session.storage_block = Some((position, Arc::from("Barrel")));
    runtime.begin_session(1);
    assert_eq!(
        super::super::presentation::forms::container_screen_reference(&player, &runtime),
        Some(Reference::ChestBarrelScreen.as_str())
    );
    assert!(!runtime.container_shows_menu(&player));

    player.begin_session(2);
    runtime.begin_session(2);
    open_storage_window(&mut runtime, &mut player, position);

    assert_eq!(
        super::super::presentation::forms::container_screen_reference(&player, &runtime),
        Some(Reference::ChestSmallChestScreen.as_str())
    );
    assert!(runtime.container_shows_menu(&player));
    assert!(!form_admitted(&mut runtime, &mut player));
}

#[test]
fn session_storage_observation_cannot_rename_the_next_servers_request_address() {
    for (block, name) in [
        ("Barrel", protocol_ids::ContainerName::Barrel),
        ("ShulkerBox", protocol_ids::ContainerName::ShulkerBox),
    ] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = in_world(&mut player, vanilla_like_table());
        let position = [3, 64, -2];
        open_storage_window(&mut runtime, &mut player, position);
        runtime.session.storage_block = Some((position, Arc::from(block)));
        assert_eq!(
            runtime
                .inventory_ledger_mut(&mut player)
                .storage_identity()
                .unwrap()
                .slot_type,
            Some(name.wire_value())
        );
        runtime.begin_session(1);
        assert_eq!(
            runtime
                .inventory_ledger_mut(&mut player)
                .storage_identity()
                .unwrap()
                .slot_type,
            Some(name.wire_value())
        );

        player.begin_session(2);
        runtime.begin_session(2);
        open_storage_window(&mut runtime, &mut player, position);

        assert_eq!(
            runtime
                .inventory_ledger_mut(&mut player)
                .storage_identity()
                .unwrap()
                .slot_type,
            Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
            "retired block {block}"
        );
    }
}

/// Storage windows answer with the variant their block entity draws, not the small chest's.
#[test]
fn form_admission_follows_the_drawn_storage_variant() {
    for (block_entity, admitted) in [(None, false), (Some("Barrel"), true)] {
        let mut player_runtime = player_state::PlayerState::new(1);
        let table = HashMap::from([
            (
                "chest.small_chest_screen",
                settings(json!({ "is_showing_menu": true })),
            ),
            (
                "chest.barrel_screen",
                settings(json!({ "is_showing_menu": false })),
            ),
        ]);
        let mut runtime = in_world(&mut player_runtime, Arc::new(ScreenSettingsTable(table)));
        let position = [3, 64, -2];
        open_storage_window(&mut runtime, &mut player_runtime, position);
        assert!(runtime.inventory_open());
        runtime.session.storage_block = block_entity.map(|id| (position, Arc::from(id)));
        assert_eq!(
            form_admitted(&mut runtime, &mut player_runtime),
            admitted,
            "block entity {block_entity:?}"
        );
    }
}

/// A form admitted over a non-menu container mid-drag ends the drag: the container never saw the
/// release, so hovering another cell after the form closes must not keep distributing.
#[test]
fn a_form_over_a_container_ends_the_drag_in_flight() {
    use crate::ui_runtime::inventory_drag::{PointerAction, PointerFrame};
    use ui_screens::container_actions::InventoryCellHit;
    let frame = |hit, primary_pressed| PointerFrame {
        primary_pressed,
        primary_released: false,
        secondary_pressed: false,
        secondary_released: false,
        shift: false,
        holding: true,
        hit: Some(hit),
        now_millis: 0,
    };
    let mut player_runtime = player_state::PlayerState::new(1);
    let table = HashMap::from([(
        "crafting.inventory_screen",
        settings(json!({ "is_showing_menu": false })),
    )]);
    let mut runtime = in_world(&mut player_runtime, Arc::new(ScreenSettingsTable(table)));
    runtime.toggle_inventory(&mut player_runtime, None);
    let pointer = &mut runtime.screen_state_mut().pointer;
    assert_eq!(
        pointer.step(frame(InventoryCellHit::Player(9), true)),
        [PointerAction::Click(InventoryCellHit::Player(9))]
    );
    assert!(form_admitted(&mut runtime, &mut player_runtime));
    // The release lands while the form owns input, then the form closes.
    let identity = runtime.server_forms().active().unwrap().identity;
    runtime
        .respond_to_server_form(identity, crate::ui_runtime::LocalFormAction::Dismiss)
        .unwrap();
    assert!(
        runtime
            .screen_state_mut()
            .pointer
            .step(frame(InventoryCellHit::Player(10), false))
            .is_empty(),
        "no distribution resumes without a held button"
    );
}

// The pack's mob effect screen absorbs gameplay input and closes when the player is hurt.
#[test]
fn carrier_mob_effect_screen_absorbs_input_and_closes_on_hurt() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let Some(carrier) = ui_engine::test_support::carrier() else {
        eprintln!(
            "skipping carrier_mob_effect_screen_absorbs_input_and_closes_on_hurt: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let catalog = Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .unwrap();
    let table = ScreenSettingsTable::for_catalog(&catalog, &Context::retail(true));
    let mut runtime = in_world(&mut player_runtime, Arc::new(table));
    runtime
        .apply_local_effect(
            runtime.session_id(),
            1,
            protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 42,
                action: protocol::ActorEffectAction::Add,
                effect_id: 1,
                amplifier: 0,
                particles: true,
                ambient: false,
                duration_ticks: -1,
                tick: 100,
            },
            0,
        )
        .unwrap();
    assert!(runtime.toggle_mob_effect_screen(0));
    assert!(!runtime.gameplay_input(&player_runtime, None));
    assert_eq!(
        runtime.scenes(&player_runtime, host(None)).closes_on_hurt(),
        Some(Scene::MobEffects)
    );
    runtime.note_player_hurt();
    runtime.close_scenes_on_player_hurt(&mut player_runtime, None);
    assert!(!runtime.mob_effect_screen().is_open());
    assert!(runtime.gameplay_input(&player_runtime, None));
}

// Shortcut adapters see every runtime screen except their own, whatever its input policy.
#[test]
fn screen_open_besides_names_every_screen_but_the_caller() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = in_world(&mut player_runtime, vanilla_like_table());
    assert!(!runtime.screen_open_besides(&player_runtime, None));
    runtime
        .apply_local_effect(
            runtime.session_id(),
            1,
            protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 42,
                action: protocol::ActorEffectAction::Add,
                effect_id: 1,
                amplifier: 0,
                particles: true,
                ambient: false,
                duration_ticks: -1,
                tick: 100,
            },
            0,
        )
        .unwrap();
    assert!(runtime.toggle_mob_effect_screen(0));
    assert!(runtime.screen_open_besides(&player_runtime, None));
    assert!(runtime.screen_open_besides(&player_runtime, Some(Scene::Emote)));
    assert!(!runtime.screen_open_besides(&player_runtime, Some(Scene::MobEffects)));
    runtime.close_mob_effect_screen();
    runtime.credits_mut().open(42, 1, 0);
    assert!(runtime.screen_open_besides(&player_runtime, Some(Scene::MobEffects)));
}
