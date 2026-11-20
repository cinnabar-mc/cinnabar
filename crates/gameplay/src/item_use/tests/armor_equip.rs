//! Right-click armor equips: one fresh press, then no held repeat.

use super::*;

const HELMET: i32 = 6;

fn empty_item() -> VerifiedNetworkItemStack {
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack::empty(),
        NetworkItemStack::empty().nbt_digest,
    )
    .unwrap()
}

fn equip(
    slot: u8,
    revision: u64,
    from: &FrozenMiningSelection,
    worn: VerifiedNetworkItemStack,
) -> ArmorEquip {
    ArmorEquip {
        slot,
        revision,
        from: from.item.clone(),
        armor_slot: 0,
        worn,
    }
}

/// The click-air transaction names the source slot and the armor legacy slot.
#[test]
fn a_fresh_press_sends_the_armor_equip_once() {
    let from = stack(3, HELMET, 1);
    let worn = stack(2, HELMET, 1).item;
    let first = item_frame(1, true, from.clone(), "minecraft:diamond_helmet");
    assert!(first.air_use.is_none(), "armor has no air use");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_armor_equip(Some(equip(3, 1, &from, worn.clone())), true);
    runtime.observe_press(true);
    let outcome = runtime.step(&first);
    assert_eq!(outcome.packets.len(), 1);
    let observed = transaction(&outcome.packets[0]);
    assert!(protocol::is_legacy_request_id(observed.legacy_request_id));
    assert_eq!(
        observed.legacy_set_slots,
        Some(vec![(protocol::CONTAINER_NAME_ARMOR, vec![0])])
    );
    let TransactionBody::ItemUse(use_observed) = observed.body else {
        panic!("an item use transaction");
    };
    assert_eq!(
        use_observed.action,
        protocol::test_support::transactions::ItemUseAction::ClickAir
    );
    let [action] = &observed.actions[..] else {
        panic!("one armor action");
    };
    assert_eq!(action.slot, 3);
    assert_eq!(action.from_item.network_id, HELMET);
    assert_eq!(action.from_item.count, 1);
    assert_eq!(action.to_item.network_id, HELMET);
    assert_eq!(action.to_item.count, 1);

    let (revision, change) = runtime
        .take_armor_change()
        .expect("the ledger receives the sent swap");
    assert_eq!(revision, 1);
    assert_eq!(change.slot, 3);
    assert_eq!(change.armor_slot, 0);
    assert_eq!(change.from, from.item);
    assert_eq!(change.to, worn);
}

/// Holding the button after the equip must not swap the pieces back; only a
/// fresh press after release equips again.
#[test]
fn a_held_repeat_does_not_equip_the_worn_piece_back() {
    let from = stack(3, HELMET, 1);
    let worn = stack(2, HELMET, 1).item;
    let empty = empty_item();
    let first = item_frame(1, true, from.clone(), "minecraft:diamond_helmet");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_armor_equip(Some(equip(3, 1, &from, worn.clone())), true);
    runtime.observe_press(true);
    assert_eq!(runtime.step(&first).packets.len(), 1);
    assert!(runtime.take_armor_change().is_some());

    // The real frame loop re-derives the plan: the source now holds the worn
    // piece and the head slot is empty.
    let swapped = FrozenMiningSelection {
        slot: 3,
        item: worn.clone(),
    };
    let held = UseFrame {
        tick: 2,
        now_millis: first.now_millis + 500,
        inventory_revision: Some(2),
        ..item_frame(2, true, swapped.clone(), "minecraft:diamond_helmet")
    };
    runtime.observe_armor_equip(Some(equip(3, 2, &swapped, empty)), true);
    assert!(
        runtime.step(&held).packets.is_empty(),
        "a held button must not swap the pieces back"
    );
    assert!(runtime.take_armor_change().is_none());

    let released = UseFrame {
        tick: 3,
        now_millis: held.now_millis + 50,
        held: false,
        ..held.clone()
    };
    assert!(runtime.step(&released).packets.is_empty());

    let pressed_again = UseFrame {
        tick: 4,
        now_millis: released.now_millis + 500,
        held: true,
        inventory_revision: Some(3),
        ..held
    };
    runtime.observe_armor_equip(Some(equip(3, 3, &swapped, empty_item())), true);
    runtime.observe_press(true);
    assert_eq!(
        runtime.step(&pressed_again).packets.len(),
        1,
        "a new press equips the worn piece again"
    );
    assert!(runtime.take_armor_change().is_some());
}

/// A plan that no longer matches the frame's source or revision is ignored.
#[test]
fn a_stale_or_mismatched_plan_sends_a_plain_air_use() {
    let from = stack(3, HELMET, 1);
    let worn = stack(2, HELMET, 1).item;
    let other = stack(2, 200, 1);
    for (plan, frame_selection) in [
        (equip(3, 2, &from, worn.clone()), from.clone()),
        (equip(4, 1, &from, worn.clone()), from.clone()),
        (equip(3, 1, &other, worn.clone()), from.clone()),
        (equip(3, 1, &from, worn.clone()), other),
    ] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_armor_equip(Some(plan), true);
        runtime.observe_press(true);
        let frame = item_frame(1, true, frame_selection, "minecraft:diamond_helmet");
        let outcome = runtime.step(&frame);
        assert_eq!(outcome.packets.len(), 1);
        assert!(runtime.take_armor_change().is_none());
    }
}

/// An item with an admitted air use never doubles as an armor equip.
#[test]
fn an_admitted_air_use_keeps_the_plan_unused() {
    let from = stack(3, HELMET, 1);
    let worn = stack(2, HELMET, 1).item;
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_armor_equip(Some(equip(3, 1, &from, worn)), false);
    runtime.observe_press(true);
    let frame = item_frame(1, true, from, "minecraft:snowball");
    assert!(frame.air_use.is_some());
    assert_eq!(runtime.step(&frame).packets.len(), 1);
    assert!(runtime.take_armor_change().is_none());
}

/// Without a prediction the plain air use still equips on the server, so a
/// held button must not repeat it and swap the pieces back.
#[test]
fn an_unpredicted_armor_use_does_not_repeat_while_held() {
    let from = stack(3, HELMET, 1);
    let first = item_frame(1, true, from.clone(), "minecraft:diamond_helmet");
    assert!(first.air_use.is_none(), "armor has no air use");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_armor_equip(None, true);
    runtime.observe_press(true);
    let outcome = runtime.step(&first);
    assert_eq!(outcome.packets.len(), 1);
    assert!(runtime.take_armor_change().is_none());

    let held = UseFrame {
        tick: 2,
        now_millis: first.now_millis + 500,
        ..item_frame(2, true, from, "minecraft:diamond_helmet")
    };
    runtime.observe_armor_equip(None, true);
    assert!(
        runtime.step(&held).packets.is_empty(),
        "a held button must not repeat the server-side equip"
    );
}
