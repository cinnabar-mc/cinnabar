use super::*;
use std::sync::{atomic::AtomicBool, mpsc};

#[test]
fn cancellation_and_expiry_wait_for_cooperative_work_to_stop() {
    for (elapsed, cancelled, expected) in [
        (Duration::ZERO, true, "cancelled"),
        (TIME_LIMIT, false, "time limit"),
    ] {
        let entered = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let (release, held) = mpsc::channel();
        let worker_entered = Arc::clone(&entered);
        let worker_stopped = Arc::clone(&stopped);
        let mut running = Running::spawn(move |control| {
            worker_entered.store(true, Ordering::Release);
            held.recv().unwrap();
            assert!(control.cancelled());
            worker_stopped.store(true, Ordering::Release);
            Err("interrupted".into())
        })
        .unwrap();
        test_time::eventually("compiler entered", || entered.load(Ordering::Acquire));
        assert!(running.poll(elapsed, cancelled).is_none());
        assert!(!stopped.load(Ordering::Acquire));
        release.send(()).unwrap();
        let mut result = None;
        test_time::eventually("compiler relinquished its thread", || {
            result = running.poll(Duration::ZERO, false);
            result.is_some()
        });
        assert!(stopped.load(Ordering::Acquire));
        assert!(result.unwrap().err().unwrap().contains(expected));
    }
}

#[test]
fn compiler_checks_its_own_deadline_without_host_polling() {
    let control = Control {
        started: Instant::now().checked_sub(TIME_LIMIT).unwrap(),
        ..Control::new()
    };
    assert!(control.cancelled());
    assert!(control.error().unwrap().contains("time limit"));
}

#[test]
fn interrupted_serialization_never_publishes_a_result() {
    let control = Control::new();
    control.stop(CANCELLED);
    assert!(CheckedWriter(&control).write_all(b"result").is_err());
}
