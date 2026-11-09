use super::pointer_paint::PaintTurn;
use std::time::{Duration, Instant};

const MICROSECOND: Duration = Duration::from_micros(1);

/// Mouse report rate and handling costs for one simulated second of sustained motion.
struct Motion {
    report_interval: Duration,
    move_cost: Duration,
    frame_cost: Duration,
}

/// What the simulated message loop produced.
struct Pumped {
    frames: u32,
    moves_ahead_of_paint: u32,
    longest_gap_between_frames: Duration,
}

/// Runs a message queue that hands out a move whenever a report arrived since the last one was
/// taken, and a paint only when no move is queued. `preempt` runs a paint right after a move.
fn pump(motion: &Motion, preempt: bool) -> Pumped {
    let start = Instant::now();
    let end = Duration::from_secs(1);
    // The first report has already arrived, so motion is under way from the start.
    let mut elapsed = motion.report_interval;
    let mut reports_taken = 0;
    let mut turn = PaintTurn::new();
    // Motion starts just after a frame finished.
    turn.frame_finished(start);
    let mut pumped = Pumped {
        frames: 0,
        moves_ahead_of_paint: 0,
        longest_gap_between_frames: Duration::ZERO,
    };
    let mut last_frame_end = Duration::ZERO;
    let mut paint = |elapsed: &mut Duration, turn: &mut PaintTurn, pumped: &mut Pumped| {
        let gap = elapsed.saturating_sub(last_frame_end);
        pumped.longest_gap_between_frames = pumped.longest_gap_between_frames.max(gap);
        *elapsed += motion.frame_cost;
        pumped.frames += 1;
        turn.frame_finished(start + *elapsed);
        last_frame_end = *elapsed;
    };
    while elapsed < end {
        let reports = elapsed.as_nanos() / motion.report_interval.as_nanos();
        if reports > reports_taken {
            reports_taken = reports;
            elapsed += motion.move_cost;
            if preempt && turn.frame_due_after_move(start + elapsed) {
                pumped.moves_ahead_of_paint += 1;
                paint(&mut elapsed, &mut turn, &mut pumped);
            }
        } else {
            paint(&mut elapsed, &mut turn, &mut pumped);
        }
    }
    let gap = elapsed.saturating_sub(last_frame_end);
    pumped.longest_gap_between_frames = pumped.longest_gap_between_frames.max(gap);
    pumped
}

/// An 8 kHz mouse whose moves take longer to handle than the time between reports.
fn saturating_mouse() -> Motion {
    Motion {
        report_interval: 125 * MICROSECOND,
        move_cost: 200 * MICROSECOND,
        frame_cost: 5_000 * MICROSECOND,
    }
}

#[test]
fn moves_outpacing_the_queue_starve_frames_without_preemption() {
    let pumped = pump(&saturating_mouse(), false);
    assert_eq!(pumped.frames, 0);
}

#[test]
fn moves_outpacing_the_queue_cannot_hold_back_frames() {
    let motion = saturating_mouse();
    let pumped = pump(&motion, true);
    assert!(pumped.frames > 100, "only {} frames ran", pumped.frames);
    // A frame runs after the first move that ends at or past the budget.
    assert!(
        pumped.longest_gap_between_frames <= Duration::from_millis(1) + motion.move_cost,
        "frames were held back for {:?}",
        pumped.longest_gap_between_frames
    );
}

#[test]
fn moves_cheaper_than_the_report_interval_leave_paints_to_the_queue() {
    let pumped = pump(
        &Motion {
            report_interval: 125 * MICROSECOND,
            move_cost: 50 * MICROSECOND,
            frame_cost: 5_000 * MICROSECOND,
        },
        true,
    );
    assert!(pumped.frames > 100, "only {} frames ran", pumped.frames);
    assert_eq!(pumped.moves_ahead_of_paint, 0);
}

#[test]
fn messages_queued_during_a_frame_are_handled_before_the_next_frame() {
    let start = Instant::now();
    let mut turn = PaintTurn::new();
    turn.frame_finished(start);
    // A move, a click and another move queued while the frame ran.
    assert!(!turn.frame_due_after_move(start + 100 * MICROSECOND));
    assert!(!turn.frame_due_after_move(start + 300 * MICROSECOND));
    assert!(turn.frame_due_after_move(start + 1_000 * MICROSECOND));
}

#[test]
fn a_long_frame_does_not_use_up_the_next_budget() {
    let start = Instant::now();
    let mut turn = PaintTurn::new();
    turn.frame_finished(start);
    assert!(turn.frame_due_after_move(start + 2_000 * MICROSECOND));
    turn.frame_finished(start + 22_000 * MICROSECOND);
    assert!(!turn.frame_due_after_move(start + 22_100 * MICROSECOND));
}

#[test]
fn moves_before_any_recorded_frame_still_reach_a_frame() {
    let start = Instant::now();
    let mut turn = PaintTurn::new();
    assert!(!turn.frame_due_after_move(start));
    assert!(turn.frame_due_after_move(start + 1_000 * MICROSECOND));
}
