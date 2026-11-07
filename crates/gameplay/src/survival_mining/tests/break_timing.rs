//! Destroy delays between blocks and the ground contact each tick mines with.

use super::*;

/// Releasing clears the delay, so a fresh press starts at once.
#[test]
fn release_clears_the_destroy_delay() {
    let torch = target([1, 1, 1], "minecraft:torch", None);
    let mut machine = DestroyMachine::default();
    held(&mut machine, &torch, Server);
    machine.step(DestroyInput::Released, STILL, Server);
    assert_eq!(
        kinds(&held(&mut machine, &torch, Server)),
        [(StartDestroy, [1, 1, 1], 1), (PredictDestroy, [1, 1, 1], 1)]
    );
}

/// After a multi-tick block, the next survival block starts on the tick the destroy
/// deadline is reached; a held Creative destroy repeats only once it has passed.
#[test]
fn the_next_block_starts_on_the_destroy_deadline_tick() {
    let shovel = Some("minecraft:golden_shovel");
    let mut machine = DestroyMachine::default();
    let dirt = target([0, 3, 0], "minecraft:dirt", shovel);
    while held(&mut machine, &dirt, Server).broken.is_none() {}
    let next = target([0, 2, 0], "minecraft:dirt", shovel);
    let started = (1u32..=20)
        .find(|_| !held(&mut machine, &next, Server).is_empty())
        .expect("the next block starts");
    assert_eq!(simulation_clock::TICK_DURATION * started, DESTROY_DELAY);

    let instant = |position| DestroyTarget {
        instant: true,
        ..target(position, "minecraft:stone", None)
    };
    let mut machine = DestroyMachine::default();
    held(&mut machine, &instant([0, 0, 0]), Server);
    let next = instant([0, -1, 0]);
    let repeated = (1u32..=20)
        .find(|_| !held(&mut machine, &next, Server).is_empty())
        .expect("the held Creative destroy repeats");
    assert_eq!(
        simulation_clock::TICK_DURATION * (repeated - 1),
        DESTROY_DELAY
    );
}

/// A tick that lands still mines at the airborne rate: its destroy step precedes movement.
#[test]
fn a_landing_tick_mines_at_the_airborne_rate() {
    let mut ticker = crate::test_support::survival_mining::ticker_with_ticks(0);
    let mut landing = crate::test_support::survival_mining::completed(101);
    landing.grounded_before_tick = false;
    landing.grounded_after_tick = true;
    ticker.enqueue_completed_physics(landing).unwrap();
    let ticks = ticker.unstepped_interaction_ticks(None);
    assert_eq!(ticks.len(), 1);
    assert!(!ticks[0].1.on_ground, "the landing tick mines airborne");
}
