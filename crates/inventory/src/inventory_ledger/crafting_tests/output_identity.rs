//! Recipe block identities retain all wire bits when projected into signed stack fields.

use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Decodes a block output with the high bit set through the production recipe catalog.
fn recipe() -> protocol::RecipeHandle {
    let mut fixture = RecipeFixture::shaped(
        "test:block_identity",
        1,
        1,
        vec![IngredientFixture::named("minecraft:oak_log", 0)],
        vec![OutputFixture {
            block_runtime_id: i32::MIN + 1,
            ..OutputFixture::new(PLANKS, 4)
        }],
    );
    fixture.net_id = 77;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    catalog.recipe(77).unwrap()
}

/// Checks both the predicted output and the outbound result declaration.
fn assert_output(ledger: &PlayerInventoryLedger, recipe: &protocol::RecipeHandle) {
    let placed = placed_outputs(ledger);
    let cursor = match placed.first() {
        Some((_, stack)) => stack.clone(),
        None => ledger
            .cursor_stack()
            .expect("crafted output on cursor")
            .clone(),
    };
    assert_eq!(cursor.network_id, recipe.output().network_id);
    assert_eq!(cursor.count, u16::from(recipe.output().count));
    assert_eq!(
        u32::from_ne_bytes(cursor.block_runtime_id.to_ne_bytes()),
        recipe.output().block_runtime_id
    );
    assert!(
        ledger
            .newest_request()
            .unwrap()
            .actions
            .iter()
            .any(|action| {
                matches!(action, StackRequestAction::CraftResultsDeprecated { results, .. }
            if results[0].block_runtime_id == recipe.output().block_runtime_id)
            })
    );
    assert!(ledger.pending_batch().unwrap().is_some());
}

#[test]
fn crafting_accepts_high_bit_block_output_identities() {
    let recipe = recipe();
    for window in [PERSONAL_INVENTORY_WINDOW_TYPE, WORKBENCH_WINDOW_TYPE] {
        for automatic in [false, true] {
            let mut ledger = ledger(window);
            if automatic {
                ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(0),
                        slot: u16::from(protocol::HOTBAR_SLOT_COUNT),
                    },
                    stack: stack(LOG, 101, 1),
                    storage_item: None,
                }));
                assert!(ledger.can_auto_craft(&recipe));
                ledger.begin_auto_craft(&recipe, false).unwrap();
                assert!(
                    ledger
                        .displayed_stack(protocol::HOTBAR_SLOT_COUNT)
                        .is_none()
                );
            } else {
                let slot = ledger.crafting_grid().slots().next().unwrap();
                ledger.apply(&craft_slot(u16::from(slot), stack(LOG, 101, 1)));
                ledger.begin_craft(&recipe, 1).unwrap();
                assert!(ledger.target_stack(InventoryTarget::Craft(slot)).is_none());
            }
            assert_output(&ledger, &recipe);
        }
    }
}

#[test]
fn station_crafting_preserves_high_bit_block_output_identities() {
    let recipe = recipe();
    let mut ledger = ledger(WORKBENCH_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_SMITHING_TABLE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    // The smithing table declares its result; the stonecutter's server derives it.
    assert_eq!(ledger.window_kind(), Some(protocol::WindowKind::Smithing));
    ledger.apply(&craft_slot(51, stack(LOG, 101, 1)));
    ledger
        .begin_screen_output(&ScreenCraft::Predicted {
            recipe_network_id: recipe.network_id(),
            output: recipe.output(),
        })
        .unwrap();
    assert_output(&ledger, &recipe);
}

/// Admits a plain shield result with the runtime timestamp the server serialized.
fn shield_recipe(tick: i64) -> protocol::RecipeHandle {
    let mut user_data = vec![0; 10];
    user_data.extend_from_slice(&tick.to_le_bytes());
    let mut fixture = RecipeFixture::shaped(
        "test:shield_data",
        1,
        1,
        vec![IngredientFixture::named("minecraft:oak_log", 0)],
        vec![OutputFixture {
            user_data,
            ..OutputFixture::new(PLANKS, 1)
        }],
    );
    fixture.net_id = 78;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    catalog.recipe(78).unwrap()
}

#[test]
fn crafting_preserves_shield_runtime_data_in_predicted_stacks_and_requests() {
    for tick in [0i64, 123] {
        let recipe = shield_recipe(tick);
        let mut expected = vec![0; 10];
        expected.extend_from_slice(&tick.to_le_bytes());
        for automatic in [false, true] {
            let mut ledger = ledger(WORKBENCH_WINDOW_TYPE);
            let mut entries = registry().entries.to_vec();
            let output = entries
                .iter_mut()
                .find(|entry| entry.network_id == PLANKS)
                .unwrap();
            output.identifier = Arc::from("minecraft:shield");
            output.negotiated_max_stack_size = Some(1);
            ledger.apply_registry(&ItemRegistryEvent {
                entries: entries.into(),
            });
            if automatic {
                ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(0),
                        slot: 9,
                    },
                    stack: stack(LOG, 101, 1),
                    storage_item: None,
                }));
                assert!(ledger.can_auto_craft(&recipe));
                ledger.begin_auto_craft(&recipe, false).unwrap();
            } else {
                let slot = ledger.crafting_grid().slots().next().unwrap();
                ledger.apply(&craft_slot(u16::from(slot), stack(LOG, 101, 1)));
                ledger.begin_craft(&recipe, 1).unwrap();
            }
            let placed = placed_outputs(&ledger);
            let output = placed
                .first()
                .map(|(_, stack)| stack)
                .or_else(|| ledger.cursor_stack())
                .unwrap();
            assert_eq!(output.extra_data.as_ref(), expected);
            assert!(ledger.newest_request().unwrap().actions.iter().any(|action| matches!(
                action, StackRequestAction::CraftResultsDeprecated { results, .. }
                if results[0].identifier.as_ref() == "minecraft:shield" && results[0].user_data.as_ref() == expected
            )));
            assert!(ledger.pending_batch().unwrap().is_some());
        }
    }
}
