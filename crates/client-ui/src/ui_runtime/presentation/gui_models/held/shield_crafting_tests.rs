//! Computed crafting results join the visible shield demand before icon addresses are retained.
use super::*;
use crate::ui_runtime::presentation::publish::{item_icons::stack_icon, prepare_shield_art};
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use sha2::Digest;
use ui_screens::inventory_capture::cache::{
    InventoryProjectionCache, ProjectedIcon, ProjectionInput, ProjectionRefresh,
};

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

#[test]
fn computed_shield_crafting_output_reduces_with_the_visible_atlas_demand() {
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
    let mut player = player_state::PlayerState::new(1);
    let mut entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == item)
        .unwrap()
        .clone();
    entry.network_id = stack(0).network_id;
    player.inventory.publish_bootstrap_inventory(
        Some(protocol::ItemRegistryEvent {
            entries: Arc::from([entry]),
        }),
        protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Server),
    );
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
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(
                        protocol::UI_INVENTORY_WINDOW_ID,
                    ),
                    slot: u16::from(*protocol::CRAFTING_INPUT_SLOTS.start()),
                },
                stack: stack(0),
                storage_item: None,
            },
        ));
    let output = stack(7);
    let mut recipe = RecipeFixture::shapeless(
        "fixture:decorated_shield",
        vec![IngredientFixture::named(item, 0)],
        vec![OutputFixture {
            user_data: output.extra_data.to_vec(),
            ..OutputFixture::new(output.network_id, output.count)
        }],
    );
    recipe.net_id = 1;
    player
        .inventory
        .enqueue_inventory_event(
            1,
            1,
            protocol::InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![recipe]).update()),
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
    let computed = ui_screens::inventory_capture::crafting_output(
        runtime.inventory_ledger(&player),
        runtime.crafting_match(&player),
    )
    .expect("the recipe must supply a computed output")
    .0;
    assert_eq!(computed.extra_data, output.extra_data);
    let resolve = |stack: &protocol::NetworkItemStack| {
        Some(client_world::CanonicalItemStack {
            shield_pattern: protocol::item_shield_pattern(&stack.extra_data).map(Arc::new),
            ..crate::test_support::fixture_item(item)
        })
    };
    ui.prepare_shield_icons(
        (0..4).map(|base| resolve(&stack(base)).unwrap().shield_pattern.unwrap()),
    );
    assert_eq!(
        ui.gui_models.shield_icons.pages().len(),
        render_model::MAX_UI_SHIELD_PAGES
    );
    let capture = |ui: &UiPresentationRuntime, cache: &mut InventoryProjectionCache| {
        cache
            .capture(
                ProjectionInput {
                    ledger: runtime.inventory_ledger(&player),
                    inventory_open: true,
                    refresh: ProjectionRefresh {
                        icons: true,
                        ..Default::default()
                    },
                    animation: Default::default(),
                },
                || runtime.crafting_match(&player),
                |stack, _| ProjectedIcon {
                    icon: stack_icon(&runtime, ui, stack, resolve(stack).unwrap(), None),
                    ..Default::default()
                },
                |_, _| None,
                |_| None,
            )
            .contents
            .unwrap()
    };
    let mut cache = InventoryProjectionCache::default();
    prepare_shield_art(&player, &runtime, &mut ui, resolve);
    let first = capture(&ui, &mut cache);
    let (icon, displayed) = first.crafting.output.as_ref().unwrap();
    assert_eq!(displayed.extra_data, output.extra_data);
    let icon = icon.unwrap();
    assert_ne!(
        icon,
        ui.item_icon(item, 0).unwrap(),
        "computed decoration must survive atlas pressure"
    );
    let pixels = ui
        .resolved_item_sprite(icon, assets::MAX_ICON_SIDE)
        .unwrap();
    let expected = assets::banner::color_rgb(7);
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
    prepare_shield_art(&player, &runtime, &mut ui, resolve);
    let repeated = capture(&ui, &mut cache);
    assert_eq!(first.crafting, repeated.crafting);
    assert_eq!(first.inventory_icons, repeated.inventory_icons);
}
