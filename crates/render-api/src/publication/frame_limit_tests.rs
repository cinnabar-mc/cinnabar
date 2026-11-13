//! Per-frame limits of the phone configuration and payloads larger than a frame.

use super::*;

const MOBILE: PublicationServiceConfig = PublicationServiceConfig::MOBILE;
const DESKTOP: PublicationServiceConfig = PublicationServiceConfig::DESKTOP;

/// Opens a frame holding as much elapsed credit as the configuration can bank.
fn begin_full_frame(
    allowance: &PublicationAllowance,
    config: PublicationServiceConfig,
    frame: u64,
) {
    allowance.begin_frame(
        frame,
        config.maximum_burst_items,
        config.maximum_burst_bytes,
        config.maximum_zero_byte_operations_per_frame,
        config.maximum_frame_items,
    );
}

/// Phones and tablets get the phone limits, and no per-frame phone limit exceeds desktop's.
#[test]
fn the_platform_selects_phone_limits_that_stay_within_desktop_limits() {
    assert_eq!(PublicationServiceConfig::for_platform(true), MOBILE);
    assert_eq!(PublicationServiceConfig::for_platform(false), DESKTOP);
    assert_eq!(
        PublicationServiceConfig::DEFAULT,
        PublicationServiceConfig::for_platform(cfg!(any(target_os = "android", target_os = "ios")))
    );

    assert!(MOBILE.maximum_frame_items < DESKTOP.maximum_frame_items);
    assert!(MOBILE.maximum_frame_bytes < DESKTOP.maximum_frame_bytes);
    assert!(MOBILE.maximum_burst_bytes < DESKTOP.maximum_burst_bytes);
    assert!(
        MOBILE.maximum_frame_arena_migration_bytes < DESKTOP.maximum_frame_arena_migration_bytes
    );
    assert!(
        MOBILE.maximum_frame_authored_texture_bytes < DESKTOP.maximum_frame_authored_texture_bytes
    );
    assert!(MOBILE.target_bytes_per_second <= DESKTOP.target_bytes_per_second);
    assert!(MOBILE.frame_time_pressure.is_some());
    assert_eq!(DESKTOP.frame_time_pressure, None);
}

/// However much credit is banked, a phone frame admits at most its item cap.
#[test]
fn the_phone_configuration_caps_the_items_one_frame_admits() {
    let allowance = PublicationAllowance::new(MOBILE);
    begin_full_frame(&allowance, MOBILE, 1);

    let permits: Vec<_> = std::iter::from_fn(|| allowance.try_admit_payload(1)).collect();

    assert_eq!(permits.len(), MOBILE.maximum_frame_items);
    assert_eq!(
        allowance.remaining_items(),
        MOBILE.maximum_burst_items - MOBILE.maximum_frame_items
    );
}

/// However much credit is banked, a phone frame admits at most its byte cap, and banked
/// credit stops at the burst limit.
#[test]
fn the_phone_configuration_caps_the_bytes_one_frame_admits() {
    let allowance = PublicationAllowance::new(MOBILE);
    let payload = MOBILE.maximum_frame_bytes / 16;
    let mut admitted = 0;
    for frame in 1..=3 {
        begin_full_frame(&allowance, MOBILE, frame);
        assert!(allowance.remaining_bytes() <= MOBILE.maximum_burst_bytes);
        let before = admitted;
        // Retiring at once keeps the live window from being the limit under test.
        while let Some(permit) = allowance.try_admit_payload(payload) {
            admitted += payload;
            assert!(permit.retire());
        }
        assert_eq!(admitted - before, MOBILE.maximum_frame_bytes);
    }
}

/// A payload above the frame cap publishes in a frame of its own instead of never fitting.
#[test]
fn a_payload_larger_than_the_frame_cap_is_admitted_alone() {
    let allowance = PublicationAllowance::new(MOBILE);
    let oversized = MOBILE.maximum_frame_bytes + 1;
    begin_full_frame(&allowance, MOBILE, 1);

    let permit = allowance
        .try_admit_payload(oversized)
        .expect("an oversized payload takes an empty frame");
    assert_eq!(permit.bytes(), Some(oversized));
    assert!(allowance.try_admit_payload(1).is_none());
    assert!(allowance.try_admit_payload(oversized).is_none());

    // It stays alone until the renderer has uploaded it.
    begin_full_frame(&allowance, MOBILE, 2);
    assert!(allowance.try_admit_payload(1).is_none());
    let permit = permit.into_handoff().unwrap();
    let permit = permit.into_render_entity().unwrap();
    assert!(permit.into_gpu_prepared().unwrap().retire());

    begin_full_frame(&allowance, MOBILE, 3);
    assert!(allowance.try_admit_payload(1).is_some());
}

/// A payload the render queue could never hold is refused without holding others back, and
/// desktop, whose frame cap is that limit, admits nothing above its frame cap.
#[test]
fn a_payload_above_the_largest_admitted_size_is_refused_without_blocking_others() {
    for config in [MOBILE, DESKTOP] {
        let allowance = PublicationAllowance::new(config);
        begin_full_frame(&allowance, config, 1);
        assert!(
            allowance
                .try_admit_payload(config.maximum_payload_bytes + 1)
                .is_none()
        );
        assert!(
            allowance
                .try_admit_payload(1)
                .is_some_and(|permit| permit.retire())
        );
    }
    assert_eq!(DESKTOP.maximum_payload_bytes, DESKTOP.maximum_frame_bytes);
    assert!(
        PublicationAllowance::new(MOBILE)
            .try_admit_payload(MOBILE.maximum_payload_bytes)
            .is_none(),
        "nothing is admitted before the first frame"
    );
}

/// Ordinary payloads cannot keep an oversized one waiting: they yield until it has gone.
#[test]
fn ordinary_payloads_yield_to_a_waiting_oversized_payload() {
    let allowance = PublicationAllowance::new(MOBILE);
    let oversized = MOBILE.maximum_frame_bytes + 1;
    begin_full_frame(&allowance, MOBILE, 1);
    let earlier = allowance.try_admit_payload(1).unwrap();
    assert!(allowance.try_admit_payload(oversized).is_none());
    assert!(allowance.try_admit_payload(1).is_none());

    // Still live: the oversized payload waits and nothing else is admitted around it.
    begin_full_frame(&allowance, MOBILE, 2);
    assert!(allowance.try_admit_payload(1).is_none());
    assert!(allowance.try_admit_payload(oversized).is_none());
    assert!(earlier.retire());

    begin_full_frame(&allowance, MOBILE, 3);
    assert!(allowance.try_admit_payload(1).is_none());
    let permit = allowance
        .try_admit_payload(oversized)
        .expect("the frame is empty once earlier work retired");
    assert!(permit.retire());
}

/// A waiting oversized payload that goes stale stops holding ordinary payloads back.
#[test]
fn an_abandoned_oversized_request_releases_ordinary_payloads() {
    let allowance = PublicationAllowance::new(MOBILE);
    begin_full_frame(&allowance, MOBILE, 1);
    let earlier = allowance.try_admit_payload(1).unwrap();
    assert!(
        allowance
            .try_admit_payload(MOBILE.maximum_frame_bytes + 1)
            .is_none()
    );

    begin_full_frame(&allowance, MOBILE, 2);
    assert!(allowance.try_admit_payload(1).is_none());
    begin_full_frame(&allowance, MOBILE, 3);
    assert!(allowance.try_admit_payload(1).is_some());
    assert!(earlier.retire());
}

/// A cap lowered for one frame bounds that frame, and a payload above it still goes alone.
#[test]
fn a_lowered_frame_byte_limit_bounds_the_frame_and_still_passes_one_larger_payload() {
    let allowance = PublicationAllowance::new(MOBILE);
    let limit = MOBILE.maximum_frame_bytes / 4;
    let begin = |frame| {
        allowance.begin_frame_with_byte_limit(
            frame,
            MOBILE.maximum_burst_items,
            MOBILE.maximum_burst_bytes,
            0,
            MOBILE.maximum_frame_items,
            limit,
        );
    };
    begin(1);
    assert_eq!(allowance.frame_remaining_bytes(), limit);
    assert!(allowance.try_admit_payload(limit).unwrap().retire());
    assert!(allowance.try_admit_payload(1).is_none());

    begin(2);
    let larger = allowance
        .try_admit_payload(limit + 1)
        .expect("a payload above the lowered cap takes an empty frame");
    assert!(allowance.try_admit_payload(1).is_none());
    assert!(larger.retire());

    // A limit above the configured maximum cannot raise it.
    allowance.begin_frame_with_byte_limit(
        3,
        MOBILE.maximum_burst_items,
        MOBILE.maximum_burst_bytes,
        0,
        MOBILE.maximum_frame_items,
        u64::MAX,
    );
    assert_eq!(
        allowance.frame_remaining_bytes(),
        MOBILE.maximum_frame_bytes
    );
}

/// An oversized payload waits for a frame of byte credit rather than bypassing the rate.
#[test]
fn an_oversized_payload_waits_for_a_full_frame_of_byte_credit() {
    let allowance = PublicationAllowance::new(MOBILE);
    let oversized = MOBILE.maximum_frame_bytes + 1;
    let half = MOBILE.maximum_frame_bytes / 2;
    allowance.begin_frame(1, 1, half, 0, MOBILE.maximum_frame_items);
    assert!(allowance.try_admit_payload(oversized).is_none());

    allowance.begin_frame(2, 1, half, 0, MOBILE.maximum_frame_items);
    let permit = allowance.try_admit_payload(oversized).unwrap();
    assert_eq!(allowance.remaining_bytes(), 0);
    assert!(permit.retire());
}
