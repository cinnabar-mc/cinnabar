use super::*;

#[test]
fn capacity_survives_short_drops_and_trims_after_sustained_low_use() {
    let mut retention = CapacityRetention::default();
    for _ in 1..CapacityRetention::SHRINK_AFTER {
        assert_eq!(retention.target(1024, 8, 0), 1024);
    }
    assert_eq!(retention.target(1024, 8, 0), 8);
    assert_eq!(retention.target(8, 17, 0), 32);
    for _ in 1..CapacityRetention::SHRINK_AFTER {
        assert_eq!(retention.target(1024, 0, 0), 1024);
    }
    assert_eq!(retention.target(1024, 512, 0), 1024);
    assert_eq!(retention.target(1024, 0, 0), 1024);
}

#[test]
fn scratch_preserves_changing_output_and_releases_an_empty_peak() {
    let mut scratch = FrameScratch::<u32>::default();
    for count in [512, 33, 200, 0, 512] {
        scratch.clear();
        scratch.extend(0..count);
        scratch.finish_frame();
        assert_eq!(&scratch[..], (0..count).collect::<Vec<_>>());
    }
    let capacity = scratch.capacity();
    for frame in 1..=CapacityRetention::SHRINK_AFTER {
        scratch.clear();
        scratch.finish_frame();
        assert_eq!(
            scratch.capacity(),
            if frame == CapacityRetention::SHRINK_AFTER {
                0
            } else {
                capacity
            }
        );
    }
}

#[test]
fn cloned_publication_pools_do_not_keep_producer_storage_alive() {
    let mut pool = FrameBuffers::<u32>::default();
    let held = pool.publish_with(&[1, 2, 3], |_| {});
    let weak = Arc::downgrade(&held);
    let snapshot_pool = pool.clone();
    drop(held);
    drop(pool);
    assert!(weak.upgrade().is_none());
    assert!(snapshot_pool.retired.is_empty());
}

#[test]
fn changing_publications_preserve_held_frames_and_trim_retired_peaks() {
    let mut pool = FrameBuffers::<String>::default();
    let held = pool.publish_with(&vec!["old".into(); 512], |_| {});
    for count in [3, 64, 1, 512, 8] {
        let values = vec![count.to_string(); count];
        let frame = pool.publish_with(&values, |_| {});
        assert_eq!(&*frame, values);
        assert!(held.iter().all(|value| value == "old"));
    }
    for _ in 0..CapacityRetention::SHRINK_AFTER {
        pool.publish_with(&["small".into()], |_| {});
    }
    assert!(pool.retired.iter().all(|frame| frame.len() <= 1));
    assert!(held.iter().all(|value| value == "old"));
}
