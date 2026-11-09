use super::super::blob_cache_telemetry::{BlobCacheTelemetry, LOG_INTERVAL_MS};
use super::*;

/// Captures formatted telemetry at the tracing sink without sharing a global subscriber.
fn capture_blob_cache_logs(run: impl FnOnce(&std::path::Path)) -> String {
    let output = tempfile::NamedTempFile::new().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(output.reopen().unwrap())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        run(output.path());
    });
    std::fs::read_to_string(output.path()).unwrap()
}

/// Reads the observed sample values from ordinary INFO records at the log sink.
fn ordinary_samples(logged: &str) -> Vec<u64> {
    logged
        .lines()
        .filter(|line| line.contains("client blob cache counters"))
        .map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("hashes_classified="))
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

/// Checks the next control delivery against the full expected stats snapshot.
fn expect_stats(receiver: &mut mpsc::Receiver<NetworkControlEvent>, expected: BlobCacheStats) {
    assert!(matches!(
        receiver.try_recv().expect("changed sample is delivered"),
        NetworkControlEvent::BlobCacheTelemetry { enabled: true, stats } if stats == expected
    ));
}

/// Polls a final flush whose channel has room, without advancing a timer or sleeping.
fn flush_final<S: NetworkSession>(session: &S, events: &mpsc::Sender<NetworkControlEvent>) {
    let timers = bridge::tasks::Timers(|_| Box::pin(future::pending()));
    let mut final_delivery =
        std::pin::pin!(send_final_blob_cache_telemetry(session, events, timers));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        final_delivery.as_mut().poll(&mut context),
        std::task::Poll::Ready(true)
    );
}

#[test]
fn blob_cache_ordinary_logs_are_rate_limited_without_throttling_control_delivery() {
    let mut session = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let (events, mut receiver) = mpsc::channel(1);
    let mut telemetry = BlobCacheTelemetry::default();
    let logged = capture_blob_cache_logs(|_| {
        telemetry.emit_initial(session.stats, 0);
        for now_ms in 1..=2 * LOG_INTERVAL_MS {
            session.stats.hashes_classified = now_ms;
            telemetry.try_emit(&session, &events, now_ms);
            expect_stats(&mut receiver, session.stats);
        }
    });
    assert_eq!(
        ordinary_samples(&logged),
        [0, LOG_INTERVAL_MS, 2 * LOG_INTERVAL_MS],
        "initial log plus one per second"
    );
}

#[test]
fn blob_cache_full_control_queue_preserves_due_log_and_exact_interval_boundary() {
    let mut session = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let (events, mut receiver) = mpsc::channel(1);
    let mut telemetry = BlobCacheTelemetry::default();
    let logged = capture_blob_cache_logs(|output| {
        telemetry.emit_initial(session.stats, 0);
        events
            .try_send(NetworkControlEvent::Stopped {
                decode_error_count: 0,
            })
            .unwrap();
        session.stats.hashes_classified = 1;
        telemetry.try_emit(&session, &events, LOG_INTERVAL_MS);
        assert_eq!(
            ordinary_samples(&std::fs::read_to_string(output).unwrap()),
            [0]
        );
        assert!(matches!(
            receiver.try_recv().unwrap(),
            NetworkControlEvent::Stopped { .. }
        ));
        telemetry.try_emit(&session, &events, LOG_INTERVAL_MS);
        expect_stats(&mut receiver, session.stats);
        assert_eq!(
            ordinary_samples(&std::fs::read_to_string(output).unwrap()),
            [0, 1]
        );

        session.stats.hashes_classified = 2;
        telemetry.try_emit(&session, &events, 2 * LOG_INTERVAL_MS - 1);
        expect_stats(&mut receiver, session.stats);
        telemetry.try_emit(&session, &events, 2 * LOG_INTERVAL_MS);
        assert!(
            receiver.try_recv().is_err(),
            "unchanged stats stay deduplicated"
        );

        session.stats.hashes_classified = 3;
        telemetry.try_emit(&session, &events, 2 * LOG_INTERVAL_MS);
        expect_stats(&mut receiver, session.stats);
    });
    assert_eq!(ordinary_samples(&logged), [0, 1, 3]);
}

#[test]
fn blob_cache_source_enabled_disabled_and_dedup_behavior_is_preserved() {
    let mut enabled = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let disabled = ReadyInboundSession {
        inbound: None,
        inbound_selected: Arc::default(),
    };
    let (events, mut receiver) = mpsc::channel(1);
    let mut telemetry = BlobCacheTelemetry::default();
    let logged = capture_blob_cache_logs(|_| {
        telemetry.try_emit(&disabled, &events, 0);
        flush_final(&disabled, &events);
        assert!(receiver.try_recv().is_err());
        telemetry.try_emit(&enabled, &events, 0);
        expect_stats(&mut receiver, enabled.stats);
        telemetry.try_emit(&enabled, &events, LOG_INTERVAL_MS);
        assert!(receiver.try_recv().is_err());
        telemetry.try_emit(&disabled, &events, LOG_INTERVAL_MS);
        assert!(receiver.try_recv().is_err());
        enabled.stats.hashes_classified = 1;
        telemetry.try_emit(&enabled, &events, LOG_INTERVAL_MS);
        expect_stats(&mut receiver, enabled.stats);
    });
    assert_eq!(ordinary_samples(&logged), [0, 1]);
}

#[test]
fn blob_cache_initial_and_final_logs_bypass_cadence_with_latest_stats() {
    let mut session = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let (events, mut receiver) = mpsc::channel(2);
    let mut telemetry = BlobCacheTelemetry::default();
    let logged = capture_blob_cache_logs(|_| {
        telemetry.emit_initial(session.stats, 0);
        session.stats.hashes_classified = 1;
        telemetry.try_emit(&session, &events, 1);
        expect_stats(&mut receiver, session.stats);
        session.stats.hashes_classified = 2;
        flush_final(&session, &events);
        expect_stats(&mut receiver, session.stats);
    });
    assert_eq!(ordinary_samples(&logged), [0, 2]);
}

#[test]
fn blob_cache_warning_admission_is_independent_of_info_cadence() {
    let mut session = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let (events, mut receiver) = mpsc::channel(1);
    let mut telemetry = BlobCacheTelemetry::default();
    let logged = capture_blob_cache_logs(|_| {
        telemetry.emit_initial(session.stats, 0);
        for count in 1..=8 {
            session.stats.skipped_cached_packets = count;
            telemetry.try_emit(&session, &events, 1);
            expect_stats(&mut receiver, session.stats);
        }
        for count in 1..=8 {
            session.stats.skipped_miss_responses = count;
            telemetry.try_emit(&session, &events, 1);
            expect_stats(&mut receiver, session.stats);
        }
    });
    assert_eq!(ordinary_samples(&logged), [0]);
    let warnings = logged
        .lines()
        .filter(|line| line.contains("skipped semantically invalid client blob-cache packet"))
        .count();
    assert_eq!(warnings, 8, "each counter warns at 1, 2, 4, and 8");
}
