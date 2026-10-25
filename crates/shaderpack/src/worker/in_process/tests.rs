use super::*;
use rayon::prelude::*;
use std::sync::{Condvar, Mutex, atomic::AtomicBool, mpsc};

/// Releases held compiler tasks before their owner joins them, including after a failed assertion.
struct ReleaseTasks(Arc<(Mutex<bool>, Condvar)>);

impl Drop for ReleaseTasks {
    /// Lets every task finish before the running compiler is dropped.
    fn drop(&mut self) {
        let (released, notify) = &*self.0;
        *released.lock().unwrap() = true;
        notify.notify_all();
    }
}

#[test]
fn held_compiler_programs_leave_the_shared_render_pool_available() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_gate = Arc::clone(&gate);
    let (entered, started) = mpsc::channel();
    let mut running = Running::spawn(move |_| {
        // Occupy every translation worker as a long-running parser would.
        let workers = rayon::current_num_threads();
        (0..workers).into_par_iter().for_each(|_| {
            entered.send(workers).unwrap();
            let (released, notify) = &*worker_gate;
            drop(
                notify
                    .wait_while(released.lock().unwrap(), |released| !*released)
                    .unwrap(),
            );
        });
        Err("fixture completed".into())
    })
    .unwrap();
    let release = ReleaseTasks(gate);
    let mut occupied = 0;
    test_time::eventually("compiler programs started", || {
        started.try_recv().is_ok_and(|workers| {
            occupied += 1;
            occupied == workers
        })
    });
    let (probe, available) = mpsc::channel();
    rayon::spawn(move || {
        let _ = probe.send(());
    });
    test_time::eventually("shared rendering pool remains available", || {
        available.try_recv().is_ok()
    });
    drop(release);
    test_time::eventually("compiler retired", || {
        running.poll(Duration::ZERO, false).is_some()
    });
}

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
