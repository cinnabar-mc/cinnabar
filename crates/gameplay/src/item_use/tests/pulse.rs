use super::*;

/// A pulse skips the use rearm that holds and physical presses keep, without lasting.
#[test]
fn a_pulse_skips_only_the_use_rearm() {
    for pulse in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        let first = item_frame(1, true, stack(2, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung.is_some());
        let soon = UseFrame {
            tick: 2,
            now_millis: first.now_millis + 50,
            ..first.clone()
        };
        if pulse {
            runtime.observe_pulse(true);
            assert!(runtime.pulse_pending());
            assert_eq!(runtime.press_drop_reason(&soon), None);
        } else {
            runtime.observe_press(true);
            assert_eq!(runtime.press_drop_reason(&soon), Some("rearm_pending"));
        }
        assert_eq!(runtime.step(&soon).swung.is_some(), pulse);
        assert!(!runtime.pulse_pending());
        let held = UseFrame {
            tick: 3,
            now_millis: soon.now_millis + 50,
            ..first.clone()
        };
        assert!(
            runtime.step(&held).packets.is_empty(),
            "the held repeat after a pulse keeps the rearm"
        );
    }
}

/// A pulse arrives as a held press in unordered input; marking that latched press skips the
/// rearm, and marking without a latched press creates none.
#[test]
fn a_marked_unordered_press_is_a_pulse() {
    use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
    for marked in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        let first = item_frame(1, true, stack(2, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung.is_some());
        runtime.mark_pulse();
        assert!(!runtime.pulse_pending() && !runtime.pending_edge());
        runtime.observe_unordered_use_input(
            InputAuthorityGeneration::INITIAL,
            InputSequence::new(2),
            semantic_input::ActionPhase {
                pressed: true,
                held: true,
                released: false,
            },
        );
        if marked {
            runtime.mark_pulse();
        }
        assert_eq!(runtime.pulse_pending(), marked);
        let soon = UseFrame {
            tick: 2,
            now_millis: first.now_millis + 50,
            ..first.clone()
        };
        assert_eq!(runtime.step(&soon).swung.is_some(), marked);
    }
}

/// A physical press replaces a latched pulse, so it keeps the rearm.
#[test]
fn a_physical_press_after_a_pulse_keeps_the_rearm() {
    let mut runtime = ItemUseRuntime::default();
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung.is_some());
    runtime.observe_pulse(true);
    runtime.observe_press(true);
    assert!(!runtime.pulse_pending());
    runtime.observe_pulse(false);
    assert!(!runtime.pulse_pending());
    let soon = UseFrame {
        tick: 2,
        now_millis: first.now_millis + 50,
        ..first
    };
    assert!(runtime.step(&soon).packets.is_empty());
}

/// Pulses admit one air use per slot and tick, so hopping slots between frames still throws.
#[test]
fn pulses_admit_one_air_use_per_slot_and_tick() {
    let mut runtime = ItemUseRuntime::default();
    for (tick, slot) in [(1, 2), (1, 3), (1, 2), (2, 2)] {
        let frame = item_frame(tick, false, stack(slot, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_pulse(true);
        assert!(runtime.step(&frame).swung.is_some());
        runtime.observe_pulse(true);
        assert_eq!(
            runtime.press_drop_reason(&frame),
            Some("use_tick_already_admitted")
        );
        assert!(runtime.step(&frame).packets.is_empty());
    }
}

#[test]
fn pulses_preserve_shared_item_cooldowns_across_slot_changes() {
    let mut runtime = ItemUseRuntime::default();
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:ender_pearl");
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung.is_some());
    for tick in 2..20 {
        let switched = item_frame(
            tick,
            false,
            stack((tick % 2) as u8 + 2, SNOWBALL, 16),
            "minecraft:ender_pearl",
        );
        runtime.observe_pulse(true);
        assert!(runtime.step(&switched).swung.is_none());
    }
    runtime.observe_pulse(true);
    assert!(
        runtime
            .step(&item_frame(
                21,
                false,
                stack(4, SNOWBALL, 16),
                "minecraft:ender_pearl"
            ))
            .swung
            .is_some()
    );
}

#[test]
fn a_fresh_slot_change_throw_sends_in_the_next_frame_between_ticks() {
    for tick_delta in [0, 1] {
        let mut runtime = ItemUseRuntime::default();
        let mut swings = SwingTracker::default();
        let first = item_frame(1, false, stack(0, SNOWBALL, 16), "minecraft:splash_potion");
        runtime.observe_press(true);
        let mut sent = Vec::new();
        step_and_send(&mut runtime, &mut swings, &first, 7, 6, |packets| {
            sent.extend(packets);
            Ok(())
        });
        assert_eq!(sent.len(), 2);
        sent.clear();
        let switched = UseFrame {
            tick: first.tick + tick_delta,
            now_millis: first.now_millis + 16,
            ..item_frame(1, false, stack(1, SNOWBALL, 16), "minecraft:egg")
        };
        runtime.observe_pulse(true);
        assert_eq!(runtime.press_drop_reason(&switched), None);
        step_and_send(&mut runtime, &mut swings, &switched, 7, 6, |packets| {
            sent.extend(packets);
            Ok(())
        });
        assert_eq!(sent.len(), 1, "slot 2 throws in its press frame");
        assert_eq!(
            summary(&transaction(&sent[0])).1,
            1,
            "the throw consumes an item"
        );
    }
}

#[test]
fn native_rearm_applies_to_fresh_presses_and_holds_after_slot_changes() {
    for fresh_press in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        let first = item_frame(1, true, stack(0, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung.is_some());
        let switched = UseFrame {
            tick: 2,
            now_millis: first.now_millis + 50,
            ..item_frame(2, true, stack(1, MENU_ITEM, 16), "minecraft:splash_potion")
        };
        runtime.observe_press(fresh_press);
        assert!(runtime.step(&switched).packets.is_empty());
        runtime.observe_press(fresh_press);
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 5,
                    now_millis: first.now_millis + USE_REARM_MILLIS,
                    ..switched.clone()
                })
                .packets
                .is_empty()
        );
        runtime.observe_press(fresh_press);
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 5,
                    now_millis: first.now_millis + USE_REARM_MILLIS + 1,
                    ..switched
                })
                .swung
                .is_some()
        );
    }
}

#[test]
fn chorus_fruit_cooldown_survives_release_and_reselection() {
    let mut runtime = ItemUseRuntime::default();
    let first = item_frame(1, true, stack(0, MENU_ITEM, 16), "minecraft:chorus_fruit");
    runtime.observe_press(true);
    assert!(runtime.step(&first).started);
    runtime.step(&UseFrame {
        tick: 2,
        now_millis: 100,
        held: false,
        ..first.clone()
    });
    let switched = item_frame(2, true, stack(1, MENU_ITEM, 16), "minecraft:chorus_fruit");
    runtime.observe_pulse(true);
    assert!(
        !runtime.step(&switched).started,
        "the category cooldown covers another slot"
    );
    let throw = item_frame(3, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_pulse(true);
    assert!(
        runtime.step(&throw).swung.is_some(),
        "chorus fruit does not cool down snowballs"
    );
    let cooldown = first.air_use.unwrap().cooldown().unwrap();
    let until = first.tick + u64::from(cooldown.ticks);
    runtime.observe_pulse(true);
    assert!(
        runtime
            .step(&UseFrame {
                tick: until,
                now_millis: until * 50,
                ..switched
            })
            .started
    );
}

#[test]
fn slot_change_throws_keep_cooldowns_in_their_item_categories() {
    for identifier in ["minecraft:ender_pearl", "minecraft:wind_charge"] {
        let mut runtime = ItemUseRuntime::default();
        let first = item_frame(1, false, stack(0, SNOWBALL, 16), identifier);
        let cooldown = first.air_use.unwrap().cooldown().unwrap();
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung.is_some());
        let blocked = UseFrame {
            now_millis: first.now_millis + 16,
            ..item_frame(1, false, stack(1, SNOWBALL, 16), identifier)
        };
        runtime.observe_pulse(true);
        let outcome = runtime.step(&blocked);
        assert!(outcome.swung.is_none());
        assert_eq!(kinds(&outcome), ["use"]);
        assert_eq!(summary(&transaction(&outcome.packets[0])).1, 0);
        assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 1.0);
        for (slot, other) in [
            (2, "minecraft:splash_potion"),
            (3, "minecraft:snowball"),
            (4, "minecraft:egg"),
        ] {
            let unrelated = UseFrame {
                now_millis: blocked.now_millis + u64::from(slot) * 16,
                ..item_frame(1, false, stack(slot, MENU_ITEM, 16), other)
            };
            runtime.observe_pulse(true);
            assert!(
                runtime.step(&unrelated).swung.is_some(),
                "{identifier} does not cool down {other}"
            );
        }
        let until = first.tick + u64::from(cooldown.ticks);
        let before = item_frame(until - 1, false, stack(5, SNOWBALL, 16), identifier);
        runtime.observe_pulse(true);
        assert!(runtime.step(&before).swung.is_none());
        let expired = item_frame(until, false, stack(6, SNOWBALL, 16), identifier);
        runtime.observe_pulse(true);
        assert!(runtime.step(&expired).swung.is_some());
    }
}

#[test]
fn a_rejected_chorus_fruit_use_starts_no_cooldown() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let first = item_frame(1, true, stack(0, MENU_ITEM, 16), "minecraft:chorus_fruit");
    let cooldown = first.air_use.unwrap().cooldown().unwrap();
    runtime.observe_press(true);
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &first,
        7,
        6,
        |_| { Err(BatchSendError::Full) }
    ));
    assert!(!runtime.is_using());
    assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 0.0);
    assert!(step_and_send(
        &mut runtime,
        &mut swings,
        &first,
        7,
        6,
        |_| Ok(())
    ));
    assert!(runtime.is_using());
    assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 1.0);
}
