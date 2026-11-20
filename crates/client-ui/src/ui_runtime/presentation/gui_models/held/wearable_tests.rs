//! A custom wearable held in a hand draws as its item, not as its authored attachable.
use super::*;

/// A preview runtime with `pack`'s attachables as session equipment, or as the startup catalog
/// when `startup` is set, and a sprite icon for `item`.
fn preview(pack: &assets::SessionEntityPack, item: &str, startup: bool) -> UiPresentationRuntime {
    use crate::test_support::fixture_hud;
    use ui_engine::test_support::fixture_font;
    let icons = Arc::new(
        RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog(
                pack.assets.source_manifest_sha256(),
                &[assets::IconSprite {
                    width: 16,
                    height: 16,
                    rgba8: vec![255; 1024].into(),
                }],
                &[assets::IconEntry {
                    identifier: item.into(),
                    metadata: 0,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap(),
    );
    let mut ui =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), icons).unwrap();
    if startup {
        ui.set_equipment_catalog(pack.equipment.clone());
    }
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    if !startup {
        ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
        ui.set_preview_pack_equipment(pack.equipment.clone());
    }
    ui
}

/// Session item facts declaring `item`'s `minecraft:wearable` slot, when one is given.
fn wearable_items(
    item: &str,
    wearable: Option<&str>,
) -> Arc<crate::ui_runtime::item_facts::SessionItemComponents> {
    Arc::new(
        crate::ui_runtime::item_facts::SessionItemComponents::from_iter(wearable.map(|slot| {
            (
                Arc::from(item),
                protocol::ItemComponents {
                    wearable_slot: Some(Arc::from(slot)),
                    ..protocol::ItemComponents::default()
                },
            )
        })),
    )
}

/// Whether the main hand draws the plain item sprite rather than authored geometry.
fn draws_sprite(ui: &UiPresentationRuntime) -> bool {
    ui.preview_held_models([true; 2])[0]
        .as_ref()
        .unwrap()
        .placements
        .iter()
        .all(|placement| matches!(placement, PreviewHeldPlacement::Sprite { .. }))
}

/// Dresses the preview from a player holding `item` in both hands, as each frame does.
fn dress(
    ui: &mut UiPresentationRuntime,
    runtime: &crate::ui_runtime::UiRuntime,
    world: &chunk_pipeline::WorldStream,
    item: &str,
) {
    let mut player = player_state::PlayerState::new(1);
    player.inventory.set_local_selected_slot(0);
    for window in [
        protocol::PLAYER_INVENTORY_WINDOW_ID,
        protocol::OFFHAND_WINDOW_ID,
    ] {
        player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(window),
                    slots: Arc::from([protocol::NetworkItemStack {
                        network_id: 1,
                        count: 1,
                        ..Default::default()
                    }]),
                    storage_item: Default::default(),
                },
            ));
    }
    ui.dress_player_preview(
        &player,
        runtime,
        Some(world.authority().inventory_ui()),
        Default::default(),
        |_| {
            Some(client_world::CanonicalItemStack {
                trim: None,
                identifier: Some(item.into()),
                identity: assets::ItemStackIdentity::empty(),
                visual: assets::ItemVisualRoute::Missing,
                enchanted: false,
                glint: assets::item::glint::ItemGlint::Session { enabled: false },
                use_animation: assets::item_use::ItemUseAnimation::None,
                grip: assets::item::grip::ItemGrip::Unresolved,
                map_id: None,
                shield_pattern: None,
                charged_projectile: None,
                charged_projectile_role: assets::item_use::ItemProjectileRole::None,
                damage: None,
                durability: assets::item::durability::ItemDurability::NotDamageable,
            })
        },
    );
}

#[test]
fn held_custom_wearable_preview_draws_the_item_instead_of_its_attachable() {
    let item = "fixture:z_tool";
    let pack = fixture(item, "fixture:a_tool");
    let world = owner(Arc::clone(&pack.assets));
    let mut ui = preview(&pack, item, false);
    let mut runtime = crate::ui_runtime::UiRuntime::new(1);
    // Dressing reads the session's current facts, so each change applies on its first frame.
    for (wearable, sprite) in [
        (None, false),
        (Some("slot.armor.head"), true),
        // Dropping the armor slot restores the authored model rather than a cached rejection.
        (None, false),
        (Some("slot.weapon.offhand"), false),
        (Some("slot.armor.chest"), true),
    ] {
        runtime.set_session_items(Some(wearable_items(item, wearable)));
        dress(&mut ui, &runtime, &world, item);
        assert_eq!(draws_sprite(&ui), sprite, "wearable slot {wearable:?}");
        if !sprite {
            assert_model(&ui, &pack, 0, "mapped");
        }
    }
}

#[test]
fn held_custom_wearable_startup_model_draws_the_item_instead_of_its_attachable() {
    let item = "fixture:z_tool";
    let pack = fixture(item, "fixture:a_tool");
    let mut ui = preview(&pack, item, true);
    for (wearable, sprite) in [
        (None, false),
        (Some("slot.armor.head"), true),
        (None, false),
    ] {
        ui.gui_models
            .trim_icons
            .observe_session_items(Some(&wearable_items(item, wearable)));
        ui.set_player_preview_gear([None; 4], Some((item, 0)));
        assert_eq!(draws_sprite(&ui), sprite, "wearable slot {wearable:?}");
    }
}
