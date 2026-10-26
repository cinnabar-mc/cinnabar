//! Retained creative rows follow every input they were built from, and only those.

use std::sync::Arc;

use json_ui::{CollectionItem, DataSource};
use protocol::{CreativeContentEvent, PlayerGameMode};
use ui_screens::container_actions::SEARCH_TAB;
use ui_screens::recipe_book::{BookCache, book_data};

use super::super::containers::{ContainerAuthority, container_frame};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::HudFrame;

/// Animated artwork updates the icon table while immutable creative rows keep their storage.
#[test]
fn animated_creative_icons_reuse_rows_and_publish_current_artwork() {
    const ENTRIES: usize = 256;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime =
        crate::test_support::creative_with(&mut player, u32::try_from(ENTRIES).unwrap());
    runtime.screen_state_mut().select_tab(SEARCH_TAB);
    let original = ui::IconRef {
        page: 3,
        uv: [0, 0, 4, 4],
        color_mask: None,
        glint: false,
    };
    let prefix = [original; 2];
    let mut frame = HudFrame::default();
    // Entries past the book's rows must not shift the inventory's following icon indices.
    frame.window_icons.book_entries = vec![Some(original); ENTRIES + 1];
    frame.window_icons.book_entries[1] = None;
    frame.window_icons.book_entries[ENTRIES - 1] = None;
    let source = ContainerAuthority::new(&player, &runtime);
    let mut cache = None;
    book_data(
        &source,
        &mut DataSource::new(),
        &container_frame(&frame),
        &mut prefix.to_vec(),
        true,
        &mut cache,
    );
    let rows = Arc::clone(cache.as_ref().unwrap().rows());
    assert_eq!(rows.len(), ENTRIES);
    let target = 25;
    let table_index = prefix.len() + target - 1;
    for page in [4, 5, 3] {
        let animated = ui::IconRef {
            page,
            uv: [4, 0, 8, 4],
            glint: true,
            ..original
        };
        frame.window_icons.book_entries[target] = Some(animated);
        let mut icons = Vec::with_capacity(ENTRIES + prefix.len());
        icons.extend_from_slice(&prefix);
        let ((), allocations) = test_alloc::count(|| {
            book_data(
                &source,
                &mut DataSource::new(),
                &container_frame(&frame),
                &mut icons,
                true,
                &mut cache,
            );
        });
        assert!(
            allocations < ENTRIES / 4,
            "animation must not rebuild creative rows: {allocations} allocations"
        );
        assert!(Arc::ptr_eq(&rows, cache.as_ref().unwrap().rows()));
        assert_eq!(&icons[..prefix.len()], &prefix);
        assert_eq!(icons.len(), prefix.len() + ENTRIES - 2);
        assert_eq!(icons[table_index], animated);
        assert_eq!(
            rows[target].values["#item_renderer_data"],
            json_ui::Scalar::Num(table_index as f64)
        );
    }
}

/// A newly missing icon changes table positions and must rebuild the rows that address them.
#[test]
fn creative_icon_availability_refreshes_renderer_indices() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::creative_with(&mut player, 4);
    runtime.screen_state_mut().select_tab(SEARCH_TAB);
    let icon = ui::IconRef {
        page: 3,
        uv: [0, 0, 4, 4],
        color_mask: None,
        glint: false,
    };
    let mut frame = HudFrame::default();
    frame.window_icons.book_entries = vec![Some(icon), Some(icon), None, Some(icon)];
    let source = ContainerAuthority::new(&player, &runtime);
    let mut cache = None;
    book_data(
        &source,
        &mut DataSource::new(),
        &container_frame(&frame),
        &mut Vec::new(),
        true,
        &mut cache,
    );
    let before = Arc::clone(cache.as_ref().unwrap().rows());
    assert_eq!(
        before[3].values["#item_renderer_data"],
        json_ui::Scalar::Num(2.0)
    );
    frame.window_icons.book_entries[1] = None;
    let mut icons = Vec::new();
    book_data(
        &source,
        &mut DataSource::new(),
        &container_frame(&frame),
        &mut icons,
        true,
        &mut cache,
    );
    let after = cache.as_ref().unwrap().rows();
    assert!(!Arc::ptr_eq(&before, after));
    assert_eq!(
        after[3].values["#item_renderer_data"],
        json_ui::Scalar::Num(1.0)
    );
    assert_eq!(icons, [icon; 2]);
}

#[test]
fn prewarmed_recipe_rows_follow_saved_tab_before_controller_capture() {
    use protocol::{CreativeCategory, CreativeGroup, InventoryOptions, InventoryTab};
    for (tab, expected) in [(InventoryTab::Nature, 0), (InventoryTab::Construction, 1)] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        player.inventory.set_inventory_options(InventoryOptions {
            left_tab: tab,
            ..Default::default()
        });
        runtime
            .inventory_ledger_mut(&mut player)
            .apply(&protocol::InventoryEvent::Creative(CreativeContentEvent {
                groups: Arc::from([CreativeGroup {
                    category: CreativeCategory::Construction,
                    name: "Building blocks".into(),
                    icon: None,
                }]),
                items: (1..=2)
                    .map(|id| protocol::CreativeItem {
                        creative_network_id: id,
                        stack: protocol::NetworkItemStack {
                            network_id: id as i32,
                            count: 1,
                            ..protocol::NetworkItemStack::empty()
                        },
                        group: 0,
                    })
                    .collect(),
                skipped: 0,
            }));
        assert!(runtime.screen_state().inventory.is_none());
        let frame = HudFrame::default();
        let mut cache = None;
        book_data(
            &ContainerAuthority::new(&player, &runtime),
            &mut DataSource::new(),
            &container_frame(&frame),
            &mut Vec::new(),
            true,
            &mut cache,
        );
        runtime.toggle_inventory(&mut player, None);
        assert_eq!(
            runtime
                .inventory_controller(&player)
                .unwrap()
                .options
                .left_tab,
            tab
        );
        book_data(
            &ContainerAuthority::new(&player, &runtime),
            &mut DataSource::new(),
            &container_frame(&frame),
            &mut Vec::new(),
            true,
            &mut cache,
        );
        assert_eq!(
            cache.as_ref().unwrap().row_count(),
            expected,
            "saved tab {tab:?}"
        );
        assert_eq!(
            crate::ui_runtime::inventory_actions::recipe_book_entries(&player, &runtime).len(),
            expected,
        );
    }
}

/// Scroll and pointer changes reuse rows; tab, group, catalog and icon-slot changes invalidate them.
#[test]
fn creative_rows_follow_their_inputs() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    runtime.screen_state_mut().select_tab(SEARCH_TAB);
    runtime.screen_state_mut().search = "stone".into();
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    let catalog = CreativeContentEvent {
        groups: Arc::from([]),
        items: Arc::from([]),
        skipped: 0,
    };
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&protocol::InventoryEvent::Creative(catalog.clone()));
    let frame = HudFrame::default();
    let items = Arc::from([CollectionItem::default()]);
    let cache = BookCache::capture(
        &ContainerAuthority::new(&player_runtime, &runtime),
        &container_frame(&frame),
        0,
        &items,
    );
    let reuse =
        |player_runtime: &player_state::PlayerState, runtime: &UiRuntime, frame: &HudFrame| {
            BookCache::reuse(
                &cache,
                &ContainerAuthority::new(player_runtime, runtime),
                &container_frame(frame),
                &mut Vec::new(),
                &mut DataSource::new(),
            )
        };
    runtime
        .screen_state_mut()
        .container_scroll
        .insert("grid".into(), 60.0);
    assert!(reuse(&player_runtime, &runtime, &frame));
    runtime.screen_state_mut().search.push('s');
    assert!(!reuse(&player_runtime, &runtime, &frame));
    runtime.screen_state_mut().search.pop();
    assert!(reuse(&player_runtime, &runtime, &frame));
    runtime.screen_state_mut().creative_expanded.insert(1);
    assert!(!reuse(&player_runtime, &runtime, &frame));
    runtime.screen_state_mut().creative_expanded.clear();
    runtime.screen_state_mut().select_tab(0);
    assert!(!reuse(&player_runtime, &runtime, &frame));
    runtime.screen_state_mut().select_tab(SEARCH_TAB);
    let mut other_frame = frame.clone();
    other_frame.window_icons.book_entries.push(None);
    assert!(!reuse(&player_runtime, &runtime, &other_frame));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: Arc::from([]),
        });
    assert!(!reuse(&player_runtime, &runtime, &frame));
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&protocol::InventoryEvent::Creative(CreativeContentEvent {
            items: Arc::from([protocol::CreativeItem {
                creative_network_id: 1,
                stack: protocol::NetworkItemStack::empty(),
                group: 0,
            }]),
            ..catalog
        }));
    assert!(!reuse(&player_runtime, &runtime, &frame));
}

/// A name replacement refreshes published rows even when the query and icons stay unchanged.
#[test]
fn display_name_replacement_refreshes_retained_creative_rows() {
    use crate::ui_runtime::item_facts::SessionItemComponents;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::creative_with(&mut player, 1);
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: Arc::from([protocol::ItemRegistryEntry {
                components: None,
                network_id: 1,
                identifier: "custom:blade".into(),
                component_based: true,
                version: protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            }]),
        });
    runtime.set_session_items(Some(Arc::new(SessionItemComponents::from_iter([(
        Arc::from("custom:blade"),
        protocol::ItemComponents {
            display_name: Some(Arc::from("Crystal Blade")),
            ..Default::default()
        },
    )]))));
    runtime.screen_state_mut().select_tab(SEARCH_TAB);
    runtime.screen_state_mut().search = "Crystal".into();
    let mut cache = None;
    let frame = HudFrame::default();
    for expected in [1, 0] {
        let mut data = DataSource::new();
        book_data(
            &ContainerAuthority::new(&player, &runtime),
            &mut data,
            &container_frame(&frame),
            &mut Vec::new(),
            true,
            &mut cache,
        );
        assert_eq!(
            cache.as_ref().unwrap().row_count(),
            expected,
            "published rows must follow the current displayed name"
        );
        runtime.set_session_items(None);
    }
}

/// A survival book of one log recipe per registry item, with `logs` oak logs in the inventory.
fn survival_book(recipes: usize, logs: u16) -> (player_state::PlayerState, UiRuntime) {
    use protocol::test_support::recipes::{
        CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        protocol::InventoryAuthority::Server,
    );
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&registry);
    set_logs(&mut player, &mut runtime, logs);
    let fixtures = registry
        .entries
        .iter()
        .filter(|entry| !entry.identifier.contains("log"))
        .take(recipes)
        .enumerate()
        .map(|(index, entry)| RecipeFixture {
            net_id: index as u32 + 1,
            ..RecipeFixture::shapeless(
                &format!("test:row_{index}"),
                vec![IngredientFixture::named("minecraft:oak_log", 0)],
                vec![OutputFixture::new(entry.network_id, 1)],
            )
        })
        .collect();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            protocol::InventoryEvent::Recipes(CraftingDataFixture::clearing(fixtures).update()),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(1))));
    runtime.drain_pending_inventory(&mut player);
    (player, runtime)
}

/// Fills the first inventory slot with `count` oak logs, or empties it.
fn set_logs(player: &mut player_state::PlayerState, runtime: &mut UiRuntime, count: u16) {
    let log = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:oak_log")
        .unwrap()
        .network_id;
    let mut slots =
        vec![protocol::NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    if count > 0 {
        slots[0] = protocol::NetworkItemStack {
            network_id: log,
            stack_network_id: 101,
            count,
            ..protocol::NetworkItemStack::empty()
        };
    }
    runtime
        .inventory_ledger_mut(player)
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: protocol::NetworkItemStack::empty(),
            },
        ));
}

/// Unchanged survival rows are shared without per-row work; a material change rebuilds them.
#[test]
fn survival_rows_are_reused_until_what_a_row_shows_changes() {
    const RECIPES: usize = 256;
    let (mut player, mut runtime) = survival_book(RECIPES, 16);
    let frame = HudFrame::default();
    let mut cache = None;
    let publish =
        |player: &player_state::PlayerState, runtime: &UiRuntime, cache: &mut Option<BookCache>| {
            book_data(
                &ContainerAuthority::new(player, runtime),
                &mut DataSource::new(),
                &container_frame(&frame),
                &mut Vec::new(),
                true,
                cache,
            );
        };
    let background = |cache: &Option<BookCache>| {
        cache.as_ref().unwrap().rows()[0].values["#container_item_background_texture"].clone()
    };
    publish(&player, &runtime, &mut cache);
    let first = Arc::clone(cache.as_ref().unwrap().rows());
    assert_eq!(first.len(), RECIPES);
    let ((), allocations) = test_alloc::count(|| publish(&player, &runtime, &mut cache));
    assert!(Arc::ptr_eq(&first, cache.as_ref().unwrap().rows()));
    assert!(
        allocations < RECIPES / 4,
        "an unchanged book must not rebuild its rows: {allocations} allocations"
    );
    let craftable = background(&cache);
    set_logs(&mut player, &mut runtime, 0);
    publish(&player, &runtime, &mut cache);
    assert!(!Arc::ptr_eq(&first, cache.as_ref().unwrap().rows()));
    assert_ne!(background(&cache), craftable);
}
