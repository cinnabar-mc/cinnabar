use protocol::{ActorEffectAction, ActorEffectEvent};

use super::*;

/// Applies effect `effect_id` for `duration_ticks` at `now_millis`, starting the session clock.
fn add_effect(
    runtime: &mut UiRuntime,
    fifo: u64,
    effect_id: i32,
    duration_ticks: i32,
    now_millis: u64,
) {
    runtime
        .apply_local_effect(
            runtime.session_id(),
            fifo,
            ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: ActorEffectAction::Add,
                effect_id,
                amplifier: 1,
                particles: true,
                ambient: false,
                duration_ticks,
                tick: 100,
            },
            now_millis,
        )
        .unwrap();
}

#[test]
fn binding_opens_only_while_an_effect_is_active_and_toggles_closed() {
    let mut runtime = UiRuntime::new(1);
    assert!(!runtime.toggle_mob_effect_screen(0), "no effect, no screen");
    assert!(!runtime.mob_effect_screen().is_open());
    add_effect(&mut runtime, 1, 1, 20 * 30, 0);
    assert!(runtime.toggle_mob_effect_screen(0));
    assert!(runtime.mob_effect_screen().is_open());
    assert!(
        !runtime.toggle_mob_effect_screen(0),
        "a second press closes"
    );
    // Thirty seconds later the only effect has run out.
    assert!(!runtime.toggle_mob_effect_screen(31_000));
}

#[test]
fn entries_count_down_on_the_session_clock_and_keep_infinite_effects() {
    let mut runtime = UiRuntime::new(1);
    add_effect(&mut runtime, 1, 1, 20 * 90, 0);
    add_effect(&mut runtime, 2, 16, -1, 0);
    let entries = runtime.mob_effect_entries(10_000);
    let speed = entries.iter().find(|entry| entry.effect_id == 1).unwrap();
    assert_eq!(speed.remaining_ticks, Some(20 * 80));
    assert_eq!(speed.amplifier, 1);
    let night_vision = entries.iter().find(|entry| entry.effect_id == 16).unwrap();
    assert_eq!(night_vision.remaining_ticks, None);
}

#[test]
fn a_new_session_closes_the_screen() {
    let mut runtime = UiRuntime::new(1);
    add_effect(&mut runtime, 1, 1, -1, 0);
    assert!(runtime.toggle_mob_effect_screen(0));
    runtime.begin_session(2);
    assert!(!runtime.mob_effect_screen().is_open());
}
