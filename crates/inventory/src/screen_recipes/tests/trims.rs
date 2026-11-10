//! Trim selection follows the recipe and the server registry, including custom ingredient names.
use super::*;

fn smithing() -> InventorySession {
    let mut session = session();
    let mut body = Vec::new();
    for _ in 0..7 {
        uint(&mut body, 0);
    }
    uint(&mut body, 1);
    string(&mut body, "test:trim");
    for name in [
        "minecraft:stick",
        "minecraft:diamond_chestplate",
        "minecraft:apple",
    ] {
        uint(&mut body, 1);
        string(&mut body, "name");
        string(&mut body, name);
        int(&mut body, 0);
        int(&mut body, 1);
    }
    string(&mut body, "smithing_table");
    uint(&mut body, 91);
    for _ in 8..11 {
        uint(&mut body, 0);
    }
    body.push(1);
    session.crafting_authority.observe(
        1,
        2,
        &crate::InventoryAuthorityEvent::Inventory(InventoryEvent::Recipes(
            decode_recipe_update(&body).unwrap(),
        )),
    );
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(2))));
    session.crafting_authority.advance();
    let ledger = session.ledger_mut();
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: protocol::ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_SMITHING_TABLE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Trims(protocol::TrimRegistry {
        patterns: Arc::from([(Arc::from("minecraft:stick"), Arc::from("sentry"))]),
        materials: Arc::from([(Arc::from("minecraft:apple"), Arc::from("gold"))]),
    }));
    let extra = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            damage: Some(37),
            custom_name: Some(Some("Keepsake".into())),
            enchantments: Some(vec![(0, 4)]),
            ..Default::default()
        },
    )
    .unwrap();
    let registry = protocol::vanilla_item_registry();
    for (slot, name) in [
        (53, "minecraft:stick"),
        (51, "minecraft:diamond_chestplate"),
        (52, "minecraft:apple"),
    ] {
        let data = if slot == 51 {
            Arc::clone(&extra)
        } else {
            Arc::from([])
        };
        ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot,
            },
            stack: NetworkItemStack {
                network_id: registry
                    .iter()
                    .find(|e| e.identifier.as_ref() == name)
                    .unwrap()
                    .network_id,
                count: 1,
                stack_network_id: i32::from(slot),
                nbt_digest: sha2::Sha256::digest(&data).into(),
                extra_data: data,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    }
    session
}

#[test]
fn trim_recipes_preview_the_decorated_base_without_a_static_output() {
    use sha2::Digest;
    let session = smithing();
    let recipe = session
        .active_screen_recipe(None)
        .expect("trim recipe selected");
    assert_eq!(recipe.kind, ScreenRecipeKind::SmithingTrim);
    assert_eq!(recipe.id, 91);
    assert_eq!(recipe.output, None);
    let output = session.predicted_smithing_trim().unwrap();
    assert_eq!(
        protocol::item_trim(&output.extra_data),
        Some(protocol::ItemTrim {
            material: "gold".into(),
            pattern: "sentry".into()
        })
    );
    assert_eq!(protocol::item_extra_damage(&output.extra_data), Some(37));
    assert_eq!(
        protocol::item_custom_name(&output.extra_data).as_deref(),
        Some("Keepsake")
    );
    assert_eq!(
        protocol::item_display(&output.extra_data).enchantments,
        [(0, 4)]
    );
    let digest: [u8; 32] = sha2::Sha256::digest(&output.extra_data).into();
    assert_eq!(output.nbt_digest, digest);
    assert_eq!(output.count, 1);
    assert_eq!(
        output.network_id,
        session
            .ledger()
            .target_stack(InventoryTarget::Craft(51))
            .unwrap()
            .network_id
    );
}

#[test]
fn matching_existing_trims_and_replaced_registries_do_not_offer_a_craft() {
    let mut session = smithing();
    let mut trimmed = session.predicted_smithing_trim().unwrap();
    trimmed.stack_network_id = 51;
    session
        .ledger_mut()
        .apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: 51,
            },
            stack: trimmed,
            storage_item: None,
        }));
    assert!(session.active_screen_recipe(None).is_none());
    session
        .ledger_mut()
        .apply(&InventoryEvent::Trims(protocol::TrimRegistry {
            patterns: Arc::from([(Arc::from("minecraft:stick"), Arc::from("coast"))]),
            materials: Arc::from([(Arc::from("minecraft:apple"), Arc::from("gold"))]),
        }));
    assert_eq!(
        protocol::item_trim(&session.predicted_smithing_trim().unwrap().extra_data)
            .unwrap()
            .pattern
            .as_ref(),
        "coast"
    );
    session
        .ledger_mut()
        .apply(&InventoryEvent::Trims(Default::default()));
    assert!(session.active_screen_recipe(None).is_none());
    session.begin_session(2);
    assert!(session.ledger().trim_registry().patterns.is_empty());
}
