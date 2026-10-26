use super::*;
use assets::{IconEntry, IconSprite};

/// Encodes an original stack fixture with an integer wear tag.
fn stack(damage: u32) -> protocol::NetworkItemStack {
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 3, 6, 0];
    bytes.extend_from_slice(b"Damage");
    bytes.extend_from_slice(&(damage as i32).to_le_bytes());
    bytes.push(0);
    protocol::NetworkItemStack {
        network_id: 1,
        metadata: 0,
        count: 1,
        extra_data: bytes.into(),
        ..protocol::NetworkItemStack::empty()
    }
}

#[test]
fn elytra_cells_select_worn_artwork_from_stack_damage() {
    let identifier = assets::facts::ELYTRA_IDENTIFIER;
    let sprites = [[31, 47, 59, 255], [79, 97, 113, 255]].map(|pixel| IconSprite {
        width: 1,
        height: 1,
        rgba8: pixel.to_vec().into(),
    });
    let entries = [0, 1].map(|variant| IconEntry {
        identifier: identifier.into(),
        metadata: variant,
        sprite: variant,
    });
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([1; 32], &sprites, &entries).unwrap(),
    )
    .unwrap();
    let presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let runtime = UiRuntime::new(1);
    let durability = assets::item::durability::builtin_item_durability(identifier);
    let maximum = durability.maximum().unwrap();
    for (damage, expected) in [
        (0, 0),
        (1, 0),
        (maximum - 2, 0),
        (maximum - 1, 1),
        (maximum, 1),
    ] {
        let stack = stack(damage);
        let item = client_world::CanonicalItemStack {
            use_animation: assets::item_use::ItemUseAnimation::None,
            grip: Default::default(),
            glint: Default::default(),
            identity: assets::ItemStackIdentity::empty(),
            identifier: Some(identifier.into()),
            visual: assets::ItemVisualRoute::Missing,
            charged_projectile: None,
            map_id: None,
            charged_projectile_role: assets::item_use::ItemProjectileRole::None,
            damage: Some(damage),
            durability,
            enchanted: false,
        };
        assert_eq!(
            stack_icon(&runtime, &presentation, &stack, item, None),
            presentation.item_icon(identifier, expected),
            "damage {damage}",
        );
    }
}
