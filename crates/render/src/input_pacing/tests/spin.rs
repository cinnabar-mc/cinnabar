//! The focused desktop spin covers recent sleep overshoot without letting rare hiccups hold it;
//! phones never spin.

use super::*;

fn us(micros: u64) -> Duration {
    Duration::from_micros(micros)
}

/// Records `count` copies of `overshoot`.
fn record(estimate: &mut SpinEstimate, overshoot: Duration, count: usize) {
    for _ in 0..count {
        estimate.record(overshoot);
    }
}

#[test]
fn first_wakes_cover_every_overshoot_seen() {
    let mut estimate = SpinEstimate::default();
    assert_eq!(estimate.spin, INITIAL_SPIN);
    estimate.record(us(300));
    estimate.record(us(10));
    assert_eq!(estimate.spin, us(300) + SPIN_HEADROOM);
}

#[test]
fn rare_late_wakes_do_not_hold_a_long_spin() {
    let mut estimate = SpinEstimate::default();
    record(
        &mut estimate,
        us(20),
        OVERSHOOT_WINDOW - UNCOVERED_OVERSHOOTS,
    );
    record(&mut estimate, us(450), UNCOVERED_OVERSHOOTS);
    assert_eq!(estimate.spin, MIN_SPIN);
}

#[test]
fn a_burst_of_late_wakes_raises_the_spin_until_it_leaves_the_window() {
    let burst = UNCOVERED_OVERSHOOTS + 1;
    let mut estimate = SpinEstimate::default();
    record(&mut estimate, us(20), OVERSHOOT_WINDOW);
    record(&mut estimate, us(300), burst);
    assert_eq!(estimate.spin, us(300) + SPIN_HEADROOM);
    record(&mut estimate, us(20), OVERSHOOT_WINDOW - burst);
    assert_eq!(
        estimate.spin,
        us(300) + SPIN_HEADROOM,
        "the whole burst is still in the window"
    );
    estimate.record(us(20));
    assert_eq!(estimate.spin, MIN_SPIN);
}

#[test]
fn coarse_timer_granularity_keeps_the_full_spin() {
    let mut estimate = SpinEstimate::default();
    // Wakes rounded up to a 0.5 ms timer tick overshoot evenly across the tick.
    for index in 0..OVERSHOOT_WINDOW as u64 {
        estimate.record(us(index * 500 / OVERSHOOT_WINDOW as u64));
    }
    assert_eq!(estimate.spin, MAX_SPIN);
    record(&mut estimate, Duration::from_millis(2), OVERSHOOT_WINDOW);
    assert_eq!(estimate.spin, MAX_SPIN);
}

fn sleep(duration: Duration, final_sleep: bool) -> WaitStep {
    WaitStep::Sleep {
        duration,
        final_sleep,
    }
}

/// Focused desktop waits sleep in service slices, then spin the measured window.
#[test]
fn focused_desktop_waits_spin_only_inside_the_measured_window() {
    let spin = us(300);
    assert_eq!(
        wait_step(false, true, spin, us(5_000)),
        sleep(SERVICE_SLICE, false)
    );
    assert_eq!(
        wait_step(false, true, spin, us(1_100)),
        sleep(us(800), true)
    );
    for remaining in [us(1), us(150), spin] {
        assert_eq!(wait_step(false, true, spin, remaining), WaitStep::Spin);
    }
    assert!(measures_overshoot(false, true));
}

/// Phones never busy-wait, however close the deadline: they sleep to it in service slices.
#[test]
fn mobile_waits_never_spin() {
    for precise in [true, false] {
        for spin in [Duration::ZERO, INITIAL_SPIN, MAX_SPIN] {
            for remaining in [us(1), us(50), us(500), us(2_000), us(20_000)] {
                let WaitStep::Sleep { duration, .. } = wait_step(true, precise, spin, remaining)
                else {
                    panic!("mobile spun with {remaining:?} left");
                };
                assert!(duration <= remaining);
            }
        }
        assert!(!measures_overshoot(true, precise));
    }
    assert_eq!(
        wait_step(true, true, MAX_SPIN, us(5_000)),
        sleep(SERVICE_SLICE, false)
    );
    assert_eq!(wait_step(true, true, MAX_SPIN, us(40)), sleep(us(40), true));
}

/// Unfocused waits never spin on either platform and check in less often.
#[test]
fn background_waits_sleep_in_long_slices_on_every_platform() {
    for mobile in [true, false] {
        assert_eq!(
            wait_step(mobile, false, MAX_SPIN, us(20_000)),
            sleep(BACKGROUND_SERVICE_SLICE, false)
        );
        assert_eq!(
            wait_step(mobile, false, MAX_SPIN, us(10)),
            sleep(us(10), true)
        );
    }
    assert!(!measures_overshoot(false, false));
}
