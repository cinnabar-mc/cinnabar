//! A custom wearable held in a hand draws as its item, not as its authored attachable.
use super::*;

/// The main hand's preview model for a selected pack attachable whose item declares
/// `minecraft:wearable` in `wearable`.
fn held_with_wearable(
    pack: &assets::SessionEntityPack,
    item: &str,
    wearable: Option<&str>,
) -> (UiPresentationRuntime, player_preview::PreviewHeldModel) {
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
    ui.set_gui_models(&assets::RuntimeAssets::diagnostic(), &pack.assets)
        .unwrap();
    ui.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
    ui.set_preview_pack_equipment(pack.equipment.clone());
    let items = Arc::new(
        crate::ui_runtime::item_facts::SessionItemComponents::from_iter(wearable.map(|slot| {
            (
                Arc::from(item),
                protocol::ItemComponents {
                    wearable_slot: Some(Arc::from(slot)),
                    ..protocol::ItemComponents::default()
                },
            )
        })),
    );
    ui.gui_models.trim_icons.observe_session_items(Some(&items));
    let world = owner(Arc::clone(&pack.assets));
    ui.set_player_preview_gear_with_owner(
        [None; 4],
        Some((item, 0)),
        PreviewEquipmentOwner {
            actor: world.authority().actor(1).unwrap(),
            rig: world.authority().actor_rig(1).unwrap(),
            input: Default::default(),
        },
    );
    let held = ui.preview_held_models([true; 2])[0].clone().unwrap();
    (ui, held)
}

#[test]
fn held_custom_wearable_preview_draws_the_item_instead_of_its_attachable() {
    let item = "fixture:z_tool";
    let pack = fixture(item, "fixture:a_tool");
    // Without a wearable slot, the held binding draws its authored mesh.
    let (ui, _) = held_with_wearable(&pack, item, None);
    assert_model(&ui, &pack, 0, "mapped");
    // A non-armor wearable slot keeps the binding held.
    let (ui, _) = held_with_wearable(&pack, item, Some("slot.weapon.offhand"));
    assert_model(&ui, &pack, 0, "mapped");
    // An armor slot makes the binding armor, which draws as the plain item in a hand.
    let (_, held) = held_with_wearable(&pack, item, Some("slot.armor.head"));
    assert!(
        held.placements
            .iter()
            .all(|placement| matches!(placement, PreviewHeldPlacement::Sprite { .. })),
        "a held custom helmet drew its attachable mesh"
    );
}
