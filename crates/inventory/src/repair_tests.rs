//! Combining two damaged copies of one item in the crafting grid through the server's
//! item-repair recipe.

use protocol::test_support::recipes::multi_recipe_update;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, ItemNbtEdit, ItemRegistryEvent, NetworkItemStack,
};
use sha2::{Digest, Sha256};

use crate::{CraftGridMatch, InventorySession};

/// The server's network id for the item-repair multi recipe in these tests.
const REPAIR_RECIPE_ID: u32 = 77;

/// A registry holding a bow (a damageable item) and a stick (which is not).
fn registry() -> ItemRegistryEvent {
    let entries = protocol::vanilla_item_registry();
    ItemRegistryEvent {
        entries: ["minecraft:bow", "minecraft:stick"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let mut entry = entries
                    .iter()
                    .find(|entry| entry.identifier.as_ref() == name)
                    .unwrap_or_else(|| panic!("{name} is a vanilla item"))
                    .clone();
                entry.network_id = index as i32 + 1;
                entry
            })
            .collect::<Vec<_>>()
            .into(),
    }
}

/// A stack of one `network_id` carrying `damage` and a stable server identity.
fn item(
    network_id: i32,
    stack_id: i32,
    damage: Option<i32>,
    extra: ItemNbtEdit,
) -> NetworkItemStack {
    let extra_data = protocol::edit_item_extra(&[0; 10], &ItemNbtEdit { damage, ..extra }).unwrap();
    NetworkItemStack {
        network_id,
        count: 1,
        stack_network_id: stack_id,
        nbt_digest: Sha256::digest(&extra_data).into(),
        extra_data,
        ..NetworkItemStack::empty()
    }
}

/// Opens the personal 2x2 grid with `cells` in its slots, the repair recipe advertised when
/// `advertised`.
fn session_with(cells: [NetworkItemStack; 4], advertised: bool) -> InventorySession {
    let mut session = InventorySession::new(1);
    session.publish_bootstrap_inventory(
        Some(registry()),
        InventoryEvent::Authority(InventoryAuthority::Server),
    );
    session.publish_crafting_rules_bootstrap(Default::default());
    let ledger = session.ledger_mut();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: crate::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let first = session.ledger().crafting_grid().slots().next().unwrap();
    let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    for (offset, stack) in cells.into_iter().enumerate() {
        slots[usize::from(first) + offset] = stack;
    }
    session
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slots: slots.into(),
            storage_item: Default::default(),
        }));
    if advertised {
        session
            .enqueue_inventory_event(
                1,
                1,
                InventoryEvent::Recipes(multi_recipe_update(1, REPAIR_RECIPE_ID)),
            )
            .unwrap();
        session.synchronize_crafting_frontier(1, Some((1, 0, Some(1))));
        session.drain_pending_inventory();
    }
    session
}

/// Two bows in the first grid cells, with these damages.
fn bows(first: Option<i32>, second: Option<i32>) -> [NetworkItemStack; 4] {
    [
        item(1, 101, first, Default::default()),
        item(1, 102, second, Default::default()),
        NetworkItemStack::empty(),
        NetworkItemStack::empty(),
    ]
}

/// Reads the repaired copy's damage from a matched repair.
fn repaired_damage(matched: &CraftGridMatch) -> Option<u32> {
    let CraftGridMatch::Repair(recipe) = matched else {
        return None;
    };
    protocol::item_stack_damage(recipe.output())
}

#[test]
fn two_damaged_copies_combine_into_one_with_merged_durability() {
    let session = session_with(bows(Some(300), Some(350)), true);
    let matched = session.crafting_match();
    let max = 384;
    // Remaining 84 + 34 plus the repair bonus.
    let expected = max - (84 + 34 + max * 5 / 100);
    assert_eq!(repaired_damage(&matched), Some(expected), "{matched:?}");
    let CraftGridMatch::Repair(recipe) = matched else {
        unreachable!()
    };
    assert_eq!(recipe.output().count, 1);
    assert_eq!(recipe.output().network_id, 1);
}

#[test]
fn a_copy_without_a_damage_tag_counts_as_undamaged() {
    let session = session_with(bows(Some(100), None), true);
    let matched = session.crafting_match();
    assert_eq!(repaired_damage(&matched), Some(0), "{matched:?}");
}

#[test]
fn two_undamaged_copies_make_nothing() {
    let session = session_with(bows(None, Some(0)), true);
    assert!(!matches!(
        session.crafting_match(),
        CraftGridMatch::Repair(_)
    ));
}

#[test]
fn nothing_combines_without_the_servers_repair_recipe() {
    let session = session_with(bows(Some(10), Some(20)), false);
    assert!(!matches!(
        session.crafting_match(),
        CraftGridMatch::Repair(_)
    ));
}

#[test]
fn different_items_and_items_that_cannot_wear_do_not_combine() {
    let mut cells = bows(Some(10), Some(20));
    cells[1] = item(2, 102, Some(20), Default::default());
    let session = session_with(cells, true);
    assert!(!matches!(
        session.crafting_match(),
        CraftGridMatch::Repair(_)
    ));
    let sticks = [
        item(2, 101, Some(5), Default::default()),
        item(2, 102, Some(5), Default::default()),
        NetworkItemStack::empty(),
        NetworkItemStack::empty(),
    ];
    let session = session_with(sticks, true);
    assert!(!matches!(
        session.crafting_match(),
        CraftGridMatch::Repair(_)
    ));
}

#[test]
fn a_third_item_in_the_grid_blocks_the_combination() {
    let mut cells = bows(Some(10), Some(20));
    cells[2] = item(2, 103, None, Default::default());
    let session = session_with(cells, true);
    assert!(!matches!(
        session.crafting_match(),
        CraftGridMatch::Repair(_)
    ));
}

#[test]
fn enchanted_or_renamed_copies_are_left_to_the_server() {
    for extra in [
        ItemNbtEdit {
            enchantments: Some(vec![(17, 1)]),
            ..Default::default()
        },
        ItemNbtEdit {
            custom_name: Some(Some("Named".to_owned())),
            ..Default::default()
        },
    ] {
        let mut cells = bows(Some(10), Some(20));
        cells[0] = item(1, 101, Some(10), extra);
        let session = session_with(cells, true);
        assert!(!matches!(
            session.crafting_match(),
            CraftGridMatch::Repair(_)
        ));
    }
}

#[test]
fn taking_the_result_consumes_both_copies_with_the_repair_recipe() {
    let mut session = session_with(bows(Some(300), Some(350)), true);
    session
        .begin_crafting()
        .unwrap_or_else(|error| panic!("the repair request is admitted: {error:?}"));
    let ledger = session.ledger();
    assert_eq!(ledger.pending_request_count(), 1);
    // Both copies leave the grid and the repaired copy waits on the cursor.
    let first = ledger.crafting_grid().slots().next().unwrap();
    for offset in 0..2u8 {
        assert!(
            ledger
                .target_stack(crate::inventory_ledger::InventoryTarget::Craft(
                    first + offset
                ))
                .is_none_or(|stack| stack.count == 0),
            "slot {offset} still holds a copy"
        );
    }
    let cursor = ledger.cursor_stack().expect("the repaired copy is held");
    assert_eq!(cursor.network_id, 1);
}
