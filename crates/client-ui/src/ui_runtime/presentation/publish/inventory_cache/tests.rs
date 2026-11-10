use super::*;
use crate::ui_runtime::{
    UiRuntime,
    presentation::{
        UiPresentationRuntime,
        session_icons::{self, SessionIcon, SessionIcons},
    },
};
use protocol::{ContainerIdentity, InventoryEvent, NetworkItemStack};
use ui_screens::inventory_capture::{capture_contents, capture_decorations};

const ITEM_ID: i32 = 10_001;

/// Creates a canonical authority without external packs or network traffic.
fn stream() -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

/// Reuses a valid registry declaration while replacing its numeric and named identity.
fn registry(identifier: &str) -> protocol::ItemRegistryEvent {
    let mut entry = protocol::vanilla_item_registry()[0].clone();
    entry.network_id = ITEM_ID;
    entry.identifier = identifier.into();
    protocol::ItemRegistryEvent {
        entries: vec![entry].into(),
    }
}

/// Installs original single-pixel artwork for the two registry identities.
fn presentation() -> UiPresentationRuntime {
    let entries = ["fixture:one", "fixture:two"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| assets::IconEntry {
            identifier: name.into(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect::<Vec<_>>();
    let sprites = [1, 2].map(|value| assets::IconSprite {
        width: 1,
        height: 1,
        rgba8: vec![value; 4].into(),
    });
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([1; 32], &sprites, &entries).unwrap(),
    )
    .unwrap();
    UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap()
}

/// Supplies one stack whose numeric identity is initially absent from canonical authority.
fn player() -> player_state::PlayerState {
    let mut player = player_state::PlayerState::new(1);
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from([NetworkItemStack {
                network_id: ITEM_ID,
                count: 3,
                stack_network_id: 9,
                ..NetworkItemStack::empty()
            }]),
            storage_item: NetworkItemStack::empty(),
        }));
    player
}

/// Checks all emitted fields against the uncached production resolvers.
fn capture(
    cache: &mut InventoryCache,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    presentation: &UiPresentationRuntime,
    stream: &chunk_pipeline::WorldStream,
) -> ProjectionUpdate {
    let items = ItemResolver {
        runtime,
        presentation,
        authority: Some(stream.authority().inventory_ui()),
    };
    let update = cache.capture(player, &items, &ItemIconFrames::default());
    let expected = capture_contents(
        runtime.inventory_ledger(player),
        runtime.inventory_open(),
        || runtime.crafting_match(player),
        |stack, _| items.icon(stack, None),
        |stack| Some(runtime.localized_item_name(&items.identifier(stack)?)),
    );
    if let Some(contents) = &update.contents {
        assert_eq!(contents.inventory_icons, expected.inventory_icons);
        assert_eq!(contents.storage_icons, expected.storage_icons);
        assert_eq!(contents.crafting, expected.crafting);
    }
    if let Some(names) = &update.item_names {
        assert_eq!(*names, expected.item_names);
    }
    if let Some(decorations) = &update.decorations {
        assert_eq!(
            *decorations,
            capture_decorations(
                runtime.inventory_ledger(player),
                |stack, correction| items.durability(stack, correction),
                |stack| items.icon(stack, None)
            )
        );
    }
    update
}

#[test]
fn canonical_registry_arrival_and_replacement_invalidate_without_slot_writes() {
    let player = player();
    let mut runtime = UiRuntime::new(1);
    runtime.session.inventory_open = true;
    let presentation = presentation();
    let mut stream = stream();
    let mut cache = InventoryCache::default();
    let unknown = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert!(unknown.contents.unwrap().inventory_icons.0[0].is_none());
    assert!(unknown.item_names.unwrap().is_empty());
    let revision = presentation
        .inventory_preparation_revision
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(stream.seed_item_registry(registry("fixture:one")));
    let one = capture(&mut cache, &player, &runtime, &presentation, &stream);
    let ready_revision = presentation
        .inventory_preparation_revision
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_ne!(ready_revision, revision);
    let one_art = one.contents.unwrap();
    assert!(one_art.inventory_icons.0[0].is_some());
    assert_eq!(one.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(), "One");
    let unchanged = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert!(unchanged.contents.is_none() && unchanged.decorations.is_none());
    assert_eq!(
        presentation
            .inventory_preparation_revision
            .load(std::sync::atomic::Ordering::Relaxed),
        ready_revision
    );
    assert!(stream.seed_item_registry(registry("fixture:two")));
    let two = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert_ne!(
        one_art.inventory_icons,
        two.contents.unwrap().inventory_icons
    );
    assert_eq!(two.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(), "Two");
}

#[test]
fn language_components_artwork_and_replaced_authority_refresh_their_sources() {
    let player = player();
    let mut runtime = UiRuntime::new(1);
    runtime.session.inventory_open = true;
    let mut presentation = presentation();
    let mut stream = stream();
    assert!(stream.seed_item_registry(registry("fixture:one")));
    let mut cache = InventoryCache::default();
    let original = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    runtime.set_session_items(Some(Arc::new(
        crate::ui_runtime::item_facts::SessionItemComponents::from_iter([(
            Arc::from("fixture:one"),
            protocol::ItemComponents {
                display_name: Some(Arc::from("fixture.name")),
                ..Default::default()
            },
        )]),
    )));
    let renamed = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert!(renamed.decorations.is_none());
    assert_eq!(
        renamed.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(),
        "fixture.name"
    );
    let input = b"fixture.name=Translated\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input);
        true
    }));
    let translated = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert!(translated.decorations.is_none());
    assert_eq!(
        translated.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(),
        "Translated"
    );
    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: Arc::from("fixture:one"),
            metadata: 0,
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    let packed = capture(&mut cache, &player, &runtime, &presentation, &stream);
    assert!(packed.decorations.is_some());
    assert_ne!(
        packed.contents.unwrap().inventory_icons,
        original.inventory_icons
    );
    let mut replacement = self::stream();
    assert!(replacement.seed_item_registry(registry("fixture:two")));
    let replaced = capture(&mut cache, &player, &runtime, &presentation, &replacement);
    assert_eq!(replaced.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(), "Two");
    let next_runtime = UiRuntime::new(2);
    let next_player = player_state::PlayerState::new(2);
    let reset = capture(
        &mut cache,
        &next_player,
        &next_runtime,
        &presentation,
        &replacement,
    );
    assert!(
        reset
            .contents
            .unwrap()
            .inventory_icons
            .0
            .iter()
            .all(Option::is_none)
    );
    assert!(reset.item_names.unwrap().is_empty());
}

/// Commits a single recipe update through the owning crafting frontier.
fn recipes(player: &mut player_state::PlayerState, sequence: u64, output: i32) {
    use protocol::test_support::recipes::{
        CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
    };
    let mut recipe = RecipeFixture::shapeless(
        "fixture:recipe",
        vec![IngredientFixture::named("fixture:one", 0)],
        vec![OutputFixture::new(output, 2)],
    );
    recipe.net_id = 1;
    let update = CraftingDataFixture::clearing(vec![recipe]).update();
    player
        .inventory
        .enqueue_inventory_event(1, sequence, InventoryEvent::Recipes(update))
        .unwrap();
    player
        .inventory
        .synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
    player.inventory.drain_pending_inventory();
}

#[test]
fn first_open_and_recipe_replacement_refresh_crafting_output_before_publication() {
    let mut player = player();
    player.inventory.publish_bootstrap_inventory(
        Some(registry("fixture:one")),
        InventoryEvent::Authority(protocol::InventoryAuthority::Server),
    );
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: u16::from(*protocol::CRAFTING_INPUT_SLOTS.start()),
            },
            stack: NetworkItemStack {
                network_id: ITEM_ID,
                count: 1,
                stack_network_id: 22,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    recipes(&mut player, 1, ITEM_ID);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = presentation();
    let mut stream = stream();
    assert!(stream.seed_item_registry(registry("fixture:one")));
    let mut cache = InventoryCache::default();
    let closed = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    assert!(closed.crafting.output.is_none());
    runtime.session.inventory_open = true;
    let open = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    assert_eq!(open.crafting.output.as_ref().unwrap().1.count, 2);
    assert!(open.crafting.icons[0].is_some());
    let capture_frame = |runtime: &mut UiRuntime, presentation: &mut UiPresentationRuntime| {
        let authority = stream.authority().inventory_ui();
        presentation.dress_player_preview(
            &player,
            runtime,
            Some(authority),
            Default::default(),
            |stack| authority.canonical_item_stack(stack),
        );
        super::super::capture_hud_frame(
            &player,
            runtime,
            presentation,
            Some(authority),
            semantic_input::PerspectiveMode::FirstPerson,
            0,
            Default::default(),
        );
    };
    capture_frame(&mut runtime, &mut presentation);
    let calls =
        crate::ui_runtime::inventory_ingress::CRAFTING_MATCH_CALLS.with(std::cell::Cell::get);
    capture_frame(&mut runtime, &mut presentation);
    assert_eq!(
        crate::ui_runtime::inventory_ingress::CRAFTING_MATCH_CALLS.with(std::cell::Cell::get),
        calls,
        "a nonempty grid without shields must not rematch unchanged frames"
    );
    runtime.session.inventory_open = false;
    capture(&mut cache, &player, &runtime, &presentation, &stream);
    let icons = Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: Arc::from("fixture:one"),
            metadata: 0,
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        }],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    capture(&mut cache, &player, &runtime, &presentation, &stream);
    runtime.session.inventory_open = true;
    let refreshed = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    assert_ne!(
        refreshed.crafting.output.unwrap().0,
        open.crafting.output.as_ref().unwrap().0
    );
    recipes(&mut player, 2, ITEM_ID + 1);
    let replaced = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    assert_eq!(replaced.crafting.output.unwrap().1.network_id, ITEM_ID + 1);
    runtime.session.inventory_open = false;
    capture(&mut cache, &player, &runtime, &presentation, &stream);
    runtime.session.inventory_open = true;
    let reopen = capture(&mut cache, &player, &runtime, &presentation, &stream)
        .contents
        .unwrap();
    assert_eq!(reopen.crafting.output.unwrap().1.network_id, ITEM_ID + 1);
    let mut output_visible = true;
    for (sequence, event, visible) in [
        (
            3,
            InventoryEvent::CraftingRules(protocol::CraftingRules {
                recipes_unlock: Some(true),
                limited_crafting: Some(true),
            }),
            false,
        ),
        (
            4,
            protocol::test_support::recipes::recipe_unlock_event(0, &[]),
            false,
        ),
        (
            5,
            protocol::test_support::recipes::recipe_unlock_event(1, &["fixture:recipe"]),
            true,
        ),
        (
            6,
            protocol::test_support::recipes::recipe_unlock_event(3, &["fixture:recipe"]),
            false,
        ),
    ] {
        player
            .inventory
            .enqueue_inventory_event(1, sequence, event)
            .unwrap();
        player
            .inventory
            .synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
        player.inventory.drain_pending_inventory();
        if let Some(refreshed) =
            capture(&mut cache, &player, &runtime, &presentation, &stream).contents
        {
            output_visible = refreshed.crafting.output.is_some();
        }
        assert_eq!(output_visible, visible, "sequence {sequence}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unopened_inventory_preparation_keeps_names_without_repeating_artwork() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.set_server_ui(Some(Arc::new(
        crate::ui_runtime::presentation::forms::ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/crafting.json"]}"#.to_vec(),
                ),
                (
                    "ui/crafting.json".into(),
                    br#"{"namespace":"crafting","inventory_screen":{"type":"screen"}}"#.to_vec(),
                ),
            ]],
            ..Default::default()
        },
    )));
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from([NetworkItemStack {
                network_id: ITEM_ID,
                count: 1,
                ..NetworkItemStack::empty()
            }]),
            storage_item: NetworkItemStack::empty(),
        }));
    let mut presentation = crate::test_support::mini_engine_presentation();
    let mut stream = stream();
    assert!(stream.seed_item_registry(registry("fixture:one")));
    let mut cache = InventoryCache::default();
    let update = cache.capture(
        &player,
        &ItemResolver {
            runtime: &runtime,
            presentation: &presentation,
            authority: Some(stream.authority().inventory_ui()),
        },
        &ItemIconFrames::default(),
    );
    assert_eq!(update.item_names.unwrap()[&(ITEM_ID, 0)].as_ref(), "One");
    presentation
        .build(
            &player,
            &runtime,
            0,
            [320, 240],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(presentation.engine_inventory_preparation_pending());
    assert!(
        !presentation.inventory_preparation_needed(&player, &runtime),
        "unchanged sources must not request recipe artwork again while pending"
    );
    let update = cache.capture(
        &player,
        &ItemResolver {
            runtime: &runtime,
            presentation: &presentation,
            authority: Some(stream.authority().inventory_ui()),
        },
        &ItemIconFrames::default(),
    );
    assert!(
        update.item_names.is_none(),
        "pending preparation must retain the unchanged name projection"
    );
    test_time::eventually("closed inventory readiness", || {
        presentation
            .build(
                &player,
                &runtime,
                0,
                [320, 240],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        !presentation.engine_inventory_preparation_pending()
    });
    let update = cache.capture(
        &player,
        &ItemResolver {
            runtime: &runtime,
            presentation: &presentation,
            authority: Some(stream.authority().inventory_ui()),
        },
        &ItemIconFrames::default(),
    );
    assert!(
        update.item_names.is_none(),
        "an unopened prepared screen must keep its inputs until the real open"
    );
}
