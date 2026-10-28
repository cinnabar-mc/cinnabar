//! Computed crafting results join the visible shield demand before icon addresses are retained.
use super::*;
use crate::ui_runtime::inventory_ingress::CRAFTING_MATCH_CALLS;
use protocol::test_support::recipes::multi_recipe_update;
use sha2::Digest;

/// Encodes an original shield with a cloth base and a valid retained item-data digest.
fn stack(base: u8) -> protocol::NetworkItemStack {
    let mut extra = vec![255, 255, 1, 10, 0, 0, 3, 4, 0];
    extra.extend(b"Base");
    extra.extend(i32::from(base).to_le_bytes());
    extra.push(0);
    extra.extend([0; 16]);
    protocol::NetworkItemStack {
        network_id: 1,
        count: 1,
        stack_network_id: i32::from(base) + 1,
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..Default::default()
    }
}

/// Installs original shield sheets large enough to exhaust the full-resolution atlas.
fn presentation() -> UiPresentationRuntime {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let pack = fixture(item, "fixture:other_shield");
    let mut ui = shield_preview::presentation(false, false, false);
    ui.set_preview_pack_equipment(None);
    let catalog = ui.equipment_catalog.as_ref().unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    let textures = catalog
        .textures()
        .iter()
        .cloned()
        .map(|mut texture| {
            texture.width = side;
            texture.height = side;
            texture.rgba8 = [255; 4].repeat(usize::from(side).pow(2)).into();
            texture
        })
        .collect();
    ui.set_equipment_catalog(Some(Arc::new(
        RuntimeEquipmentCatalog::from_parts([13; 32], catalog.bindings().to_vec(), textures)
            .unwrap(),
    )));
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui
}

/// Binds the shield and banner fixture identities through the negotiated registry.
fn registry() -> protocol::ItemRegistryEvent {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let entries = protocol::vanilla_item_registry();
    protocol::ItemRegistryEvent {
        entries: [item, assets::facts::BANNER_IDENTIFIER]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let mut entry = entries
                    .iter()
                    .find(|entry| entry.identifier.as_ref() == name)
                    .unwrap()
                    .clone();
                entry.network_id = stack(0).network_id + index as i32;
                entry
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

/// Seeds the four inventory patterns and the authorities needed by preview resolution.
fn player(registry: &protocol::ItemRegistryEvent) -> player_state::PlayerState {
    let mut player = player_state::PlayerState::new(1);
    player.inventory.publish_bootstrap_inventory(
        Some(registry.clone()),
        protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Server),
    );
    player
        .inventory
        .publish_crafting_rules_bootstrap(Default::default());
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(
                    protocol::PLAYER_INVENTORY_WINDOW_ID,
                ),
                slots: (0..4).map(stack).collect::<Vec<_>>().into(),
                storage_item: Default::default(),
            },
        ));
    player
}

/// Checks the nonempty projected cutout against the fixture's full-sheet cloth color.
fn assert_pattern_color(ui: &UiPresentationRuntime, icon: ui::IconRef, base: u8) {
    let pixels = ui
        .resolved_item_sprite(icon, assets::MAX_ICON_SIDE)
        .unwrap();
    let expected = assets::banner::color_rgb(i64::from(base));
    assert!(
        pixels
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] != 0)
    );
    assert!(
        pixels
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| pixel[..3] == expected)
    );
}

#[test]
fn computed_shield_crafting_output_reduces_with_the_visible_atlas_demand() {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let mut ui = presentation();
    let registry = registry();
    let mut player = player(&registry);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(3),
                window_type: inventory::inventory_ledger::WORKBENCH_WINDOW_TYPE,
                position: [0; 3],
                runtime_entity_id: -1,
            },
        ));
    let mut slots = vec![protocol::NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    let first = player
        .inventory
        .ledger()
        .crafting_grid()
        .slots()
        .next()
        .unwrap();
    let mut plain = stack(0);
    plain.stack_network_id = 101;
    plain.extra_data = Arc::from([0; 18]);
    plain.nbt_digest = sha2::Sha256::digest(&plain.extra_data).into();
    slots[usize::from(first)] = plain;
    slots[usize::from(first) + 1] = protocol::NetworkItemStack {
        network_id: registry.entries[1].network_id,
        count: 1,
        stack_network_id: 102,
        metadata: 7,
        ..protocol::NetworkItemStack::empty()
    };
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slots: slots.into(),
                storage_item: Default::default(),
            },
        ));
    let output = stack(7);
    player
        .inventory
        .enqueue_inventory_event(
            1,
            1,
            protocol::InventoryEvent::Recipes(multi_recipe_update(200, 55)),
        )
        .unwrap();
    player
        .inventory
        .synchronize_crafting_frontier(1, Some((1, 0, Some(1))));
    player.inventory.drain_pending_inventory();
    let mut runtime = crate::ui_runtime::UiRuntime::new(1);
    runtime.session.inventory_open = true;
    assert!(
        runtime
            .inventory_ledger(&player)
            .created_output_stack()
            .is_none()
    );
    let matched = runtime.crafting_match(&player);
    assert!(
        matches!(matched, inventory::CraftGridMatch::Shield(_)),
        "expected the admitted recipe, got {matched:?}; grid: {:?}",
        runtime.inventory_ledger(&player).crafting_grid_cells()
    );
    let computed =
        ui_screens::inventory_capture::crafting_output(runtime.inventory_ledger(&player), matched)
            .expect("the recipe must supply a computed output")
            .0;
    assert_eq!(computed.extra_data, output.extra_data);
    let resolve = |stack: &protocol::NetworkItemStack| {
        let name = registry
            .entries
            .iter()
            .find(|entry| entry.network_id == stack.network_id)?
            .identifier
            .as_ref();
        Some(client_world::CanonicalItemStack {
            shield_pattern: (name == item)
                .then(|| protocol::item_shield_pattern(&stack.extra_data))
                .flatten()
                .map(Arc::new),
            ..crate::test_support::fixture_item(name)
        })
    };
    ui.prepare_shield_icons(
        (0..4).map(|base| resolve(&stack(base)).unwrap().shield_pattern.unwrap()),
    );
    assert_eq!(
        ui.gui_models.shield_icons.pages().len(),
        render_model::MAX_UI_SHIELD_PAGES
    );
    let mut world = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(world.seed_item_registry(registry));
    let capture = |runtime: &mut crate::ui_runtime::UiRuntime, ui: &mut UiPresentationRuntime| {
        let authority = world.authority().inventory_ui();
        ui.dress_player_preview(
            &player,
            runtime,
            Some(authority),
            Default::default(),
            |stack| authority.canonical_item_stack(stack),
        );
        crate::ui_runtime::presentation::publish::capture_hud_frame(
            &player,
            runtime,
            ui,
            Some(authority),
            semantic_input::PerspectiveMode::FirstPerson,
            0,
            Default::default(),
        );
    };
    let before = CRAFTING_MATCH_CALLS.with(std::cell::Cell::get);
    capture(&mut runtime, &mut ui);
    assert_eq!(CRAFTING_MATCH_CALLS.with(std::cell::Cell::get), before + 1);
    let first = ui.hud_frame.crafting.clone();
    let (icon, displayed) = first.output.as_ref().unwrap();
    assert_eq!(displayed.extra_data, output.extra_data);
    let icon = icon.unwrap();
    assert_ne!(
        icon,
        ui.item_icon(item, 0).unwrap(),
        "computed decoration must survive atlas pressure"
    );
    assert_pattern_color(&ui, icon, 7);
    let calls = CRAFTING_MATCH_CALLS.with(std::cell::Cell::get);
    capture(&mut runtime, &mut ui);
    assert_eq!(first, ui.hud_frame.crafting);
    assert_eq!(
        CRAFTING_MATCH_CALLS.with(std::cell::Cell::get),
        calls,
        "an unchanged preview and HUD frame must reuse the projected crafting result"
    );
}

#[test]
fn trading_shield_previews_reduce_with_the_visible_atlas_demand() {
    let item = assets::gui_item::SHIELD_IDENTIFIER;
    let mut ui = presentation();
    let registry = registry();
    let mut player = player(&registry);
    let trade_item = |base| {
        let stack = stack(base);
        protocol::TradeItem {
            identifier: item.into(),
            metadata: 0,
            count: 1,
            block_descriptor: None,
            extra_data: Arc::from(&stack.extra_data[..stack.extra_data.len() - 8]),
        }
    };
    let mut input = trade_item(7);
    input.metadata = u32::from(protocol::RECIPE_ANY_AUX);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Trade(protocol::TradeEvent {
            container: protocol::ContainerIdentity::window(7),
            actor_unique_id: 90,
            title: "Fixture trader".into(),
            modern: true,
            tier: 0,
            offers: Arc::from([protocol::TradeOffer {
                network_id: 91,
                inputs: [Some(input), None],
                output: trade_item(8),
                tier: 0,
                uses: 0,
                max_uses: 16,
            }]),
        }));
    let mut world = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(world.seed_item_registry(registry));
    let mut runtime = crate::ui_runtime::UiRuntime::new(1);
    runtime.session.inventory_open = true;
    ui.prepare_shield_icons(
        (0..4)
            .map(|base| Arc::new(protocol::item_shield_pattern(&stack(base).extra_data).unwrap())),
    );
    assert_eq!(
        ui.gui_models.shield_icons.pages().len(),
        render_model::MAX_UI_SHIELD_PAGES
    );
    let capture = |runtime: &mut crate::ui_runtime::UiRuntime, ui: &mut UiPresentationRuntime| {
        crate::ui_runtime::presentation::publish::capture_hud_frame(
            &player,
            runtime,
            ui,
            Some(world.authority().inventory_ui()),
            semantic_input::PerspectiveMode::FirstPerson,
            0,
            Default::default(),
        );
    };
    capture(&mut runtime, &mut ui);
    let first = ui.hud_frame.window_icons.recipe.clone();
    assert!(
        first[1].is_none(),
        "the absent second input keeps its offer position"
    );
    for (index, base) in [(0, 7), (2, 8)] {
        let icon = first[index].unwrap();
        assert_ne!(
            icon,
            ui.item_icon(item, 0).unwrap(),
            "trading decoration must survive atlas pressure"
        );
        assert_pattern_color(&ui, icon, base);
    }
    assert_ne!(first[0], first[2]);
    capture(&mut runtime, &mut ui);
    assert_eq!(first, ui.hud_frame.window_icons.recipe);
}
