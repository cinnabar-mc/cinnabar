use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

/// A stream whose next slice has retention maintenance to run.
fn stream_with_work() -> WorldStream {
    let mut stream = stream();
    stream.retention_due = true;
    stream
}

/// A service thread that cannot be scheduled must not hold the frame thread at reclaim.
#[test]
fn reclaim_does_not_wait_for_a_service_thread_that_cannot_run() {
    let mut service = WorldStreamService::spawn().unwrap();
    let shared = Arc::clone(&service.shared);
    let turns = shared.turns.lock().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    let serviced = service.reclaim().expect("the lent stream comes back");
    assert_eq!(serviced.report, WorldStreamPoll::default());
    assert_eq!(serviced.held, Duration::ZERO);
    assert!(!service.is_servicing());
    drop(turns);
}

/// A lent stream with work is polled on the service thread without the frame thread driving it.
#[test]
fn the_service_polls_a_lent_stream_until_reclaimed() {
    let mut service = WorldStreamService::spawn().unwrap();
    service.launch(stream_with_work(), [0.0; 3], 0);
    assert_eq!(service.wakes, 1);
    let shared = Arc::clone(&service.shared);
    test_time::eventually("the service polls the lent stream", || {
        shared
            .slot()
            .lent
            .as_ref()
            .is_some_and(|lent| lent.serving_since.is_some())
    });
    let serviced = service.reclaim().expect("the lent stream comes back");
    assert!(serviced.stream.between_frames_service);
    service.launch(serviced.stream, [0.0; 3], 0);
    assert!(service.reclaim().is_some());
}

/// A loan with nothing to advance leaves a sleeping service asleep; the stream still comes back
/// unserviced, so the next frame poll keeps its commits.
#[test]
fn an_idle_loan_does_not_wake_the_service() {
    let mut service = WorldStreamService::spawn().unwrap();
    let stream = stream();
    assert!(!stream.has_service_work());
    service.launch(stream, [0.0; 3], 0);
    assert_eq!(service.wakes, 0);
    assert!(service.is_servicing());
    let serviced = service.reclaim().expect("the lent stream comes back");
    assert_eq!(serviced.held, Duration::ZERO);
    assert_eq!(serviced.stream.service_window, Duration::ZERO);
    assert_eq!(serviced.report, WorldStreamPoll::default());

    // Work that appears later wakes the same service on its next loan.
    let mut stream = serviced.stream;
    stream.retention_due = true;
    service.launch(stream, [0.0; 3], 0);
    assert_eq!(service.wakes, 1);
    let shared = Arc::clone(&service.shared);
    test_time::eventually("the woken service polls the loan", || {
        shared
            .slot()
            .lent
            .as_ref()
            .is_some_and(|lent| lent.serving_since.is_some())
    });
    assert!(service.reclaim().is_some());
}

/// Dropping a service that holds a stream releases it and stops the thread.
#[test]
fn dropping_a_servicing_service_returns() {
    let mut service = WorldStreamService::spawn().unwrap();
    service.launch(stream(), [0.0; 3], 0);
    let shared = Arc::clone(&service.shared);
    drop(service);
    assert!(shared.slot().lent.is_none());
    assert!(shared.slot().shutdown);
}

/// A wake is consumed by the idle wait that observes it, so it cannot keep later waits spinning.
#[test]
fn idle_wait_consumes_the_wake_it_observes() {
    let stream = stream();
    let (sent, wake) = bounded(1);
    sent.send(()).unwrap();
    stream.idle_wait().wait(&wake, false, Duration::ZERO);
    assert!(wake.is_empty());
}
