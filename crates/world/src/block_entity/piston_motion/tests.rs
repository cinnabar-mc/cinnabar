use super::*;

/// Progress after each of the ticks `from..=to`.
fn progress(motion: &PistonMotion, from: u64, to: u64) -> Vec<f32> {
    (from..=to)
        .map(|tick| motion.step_at(tick).progress)
        .collect()
}

#[test]
fn an_expanding_arm_gains_half_a_block_per_tick_from_where_it_was() {
    let motion = PistonMotion::reported(None, EXPANDING, 10);
    assert_eq!(progress(&motion, 9, 14), [0.0, 0.0, 0.5, 1.0, 1.0, 1.0]);
    let step = motion.step_at(12);
    assert_eq!((step.last_progress, step.moved()), (0.5, true));
    assert!(!motion.step_at(13).moved());
}

#[test]
fn a_retracting_report_continues_from_the_progress_reached() {
    let expanding = PistonMotion::reported(None, EXPANDING, 10);
    let retracting = PistonMotion::reported(Some(&expanding), RETRACTING, 11);
    assert_eq!(progress(&retracting, 11, 14), [0.5, 0.0, 0.0, 0.0]);
    let extended = PistonMotion::reported(Some(&expanding), EXPANDED, 20);
    let retracting = PistonMotion::reported(Some(&extended), RETRACTING, 30);
    assert_eq!(progress(&retracting, 30, 33), [1.0, 0.5, 0.0, 0.0]);
}

#[test]
fn resting_states_snap_to_their_end_in_one_tick() {
    let extended = PistonMotion::reported(None, EXPANDED, 5);
    assert_eq!(progress(&extended, 5, 7), [0.0, 1.0, 1.0]);
    let retracted = PistonMotion::reported(Some(&extended), RETRACTED, 6);
    assert_eq!(progress(&retracted, 6, 8), [1.0, 0.0, 0.0]);
}

#[test]
fn a_report_of_the_same_state_keeps_the_motion() {
    let expanding = PistonMotion::reported(None, EXPANDING, 10);
    assert_eq!(
        PistonMotion::reported(Some(&expanding), EXPANDING, 11),
        expanding
    );
    let unknown = PistonMotion::reported(Some(&expanding), 9, 11);
    assert_eq!(progress(&unknown, 11, 13), [0.5, 0.5, 0.5]);
}

#[test]
fn rebasing_keeps_the_progress_and_local_state_reached() {
    let expanding = PistonMotion::reported(None, EXPANDING, 100);
    let rebased = expanding.rebased(101, 3);
    assert_eq!(progress(&rebased, 3, 5), [0.5, 1.0, 1.0]);
    let settled = expanding.rebased(200, 3);
    assert_eq!(progress(&settled, 3, 4), [1.0, 1.0]);
    assert!(!settled.step_at(4).moved());
}
