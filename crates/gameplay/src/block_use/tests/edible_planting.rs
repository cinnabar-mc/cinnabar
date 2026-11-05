//! Planting consumes an edible item's press before its air-use fallback runs.
use super::*;
use crate::{
    item_use::{ItemUseRuntime, UseFrame, fixtures::classify},
    mining::FrozenMiningSelection,
};

/// Supplies an edible seed whose appetite requirement is already resolved by the caller.
fn food_frame(tick: u64, identifier: &str, ready: bool, consumed: bool) -> UseFrame {
    let mut item = network_item(300, 0);
    item.count = 8;
    UseFrame {
        riptide: None,
        riptide_facing: Default::default(),
        tick,
        now_millis: tick * 50,
        position: [0.5, 65.62, 0.5],
        held: true,
        selection: Some(FrozenMiningSelection {
            slot: 0,
            item: verified(item),
        }),
        air_use: classify(identifier, false, 0, Some(32)),
        ready,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: consumed,
    }
}

#[test]
fn planting_edible_crops_does_not_start_eating_or_arm_a_held_repeat() {
    for identifier in ["minecraft:carrot", "minecraft:potato"] {
        for ready in [false, true] {
            let mut block = BlockUseRuntime::default();
            block.record(
                ItemUseTrigger::PlayerInput,
                Duration::from_millis(5_000),
                100,
                LocalUse::Place,
                clock(5_000, 0.0),
            );
            let mut item = ItemUseRuntime::default();
            item.observe_press(true);
            for tick in 100..140 {
                let frame = food_frame(tick, identifier, ready, block.press_consumed());
                assert!(frame.air_use.is_some(), "the fixture must be edible");
                let outcome = item.step(&frame);
                assert!(
                    outcome.packets.is_empty() && !outcome.started && !item.is_using(),
                    "planting {identifier} started an air use at tick {tick} (ready: {ready})"
                );
                // A later failed held planting must not arm eating for the original press.
                block.record(
                    ItemUseTrigger::SimulationTick,
                    Duration::from_millis(tick * 50),
                    tick,
                    LocalUse::Nothing,
                    clock(tick * 50, 0.0),
                );
            }
            item.step(&UseFrame {
                held: false,
                ..food_frame(140, identifier, ready, block.press_consumed())
            });
            block.forget_press_resolution();
            block.record(
                ItemUseTrigger::PlayerInput,
                Duration::from_millis(7_050),
                141,
                LocalUse::Nothing,
                clock(7_050, 0.0),
            );
            item.observe_press(true);
            let outcome = item.step(&food_frame(141, identifier, ready, block.press_consumed()));
            assert_eq!(outcome.packets.len(), 1, "a new air-use press still works");
            assert_eq!(outcome.started, ready);
            assert_eq!(item.is_using(), ready);
        }
    }
}

#[test]
fn refused_planting_does_not_consume_the_press() {
    let mut block = BlockUseRuntime::default();
    block.observe_use(true, true, false, true);
    assert!(!block.admit(
        ItemUseTrigger::PlayerInput,
        Duration::from_millis(5_000),
        100,
        LocalUse::Place,
        clock(5_000, 0.0),
        false,
    ));
    assert!(block.press_pending());
    assert!(!block.press_consumed());
}
