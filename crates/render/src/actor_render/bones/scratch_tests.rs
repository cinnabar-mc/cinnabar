use super::tests::{apply_and_compare, frame, prepare};
use super::*;

#[test]
fn warmed_preparation_reuses_storage_without_mutating_held_publications() {
    let source = frame(&(1..=200).map(|id| (id, 3)).collect::<Vec<_>>());
    let mut cache = BoneResidency::default();
    let mut gpu = [Vec::new(), Vec::new()];
    let held = prepare(&mut cache, &source);
    apply_and_compare(&cache, &held, &source, &mut gpu);
    let snapshot = held.manifest.to_vec();
    for count in [50, 200, 75, 200, 50, 200] {
        let next = frame(&(1..=count).map(|id| (id, 3)).collect::<Vec<_>>());
        let prepared = prepare(&mut cache, &next);
        apply_and_compare(&cache, &prepared, &next, &mut gpu);
        cache.recycle(prepared.previous, prepared.current);
        assert_eq!(&*held.manifest, snapshot);
    }
    for _ in 0..6 {
        let prepared = prepare(&mut cache, &source);
        cache.recycle(prepared.previous, prepared.current);
    }
    let pointers = (
        cache.scratch.assignments.as_ptr(),
        cache.scratch.keys.as_ptr(),
        cache.scratch.holes.as_ptr(),
    );
    let (prepared, allocations) = test_alloc::count(|| prepare(&mut cache, &source));
    assert_eq!(
        allocations, 0,
        "warm preparation must reuse temporary and publication storage"
    );
    assert_eq!(
        pointers,
        (
            cache.scratch.assignments.as_ptr(),
            cache.scratch.keys.as_ptr(),
            cache.scratch.holes.as_ptr()
        )
    );
    assert!(prepared.previous.is_empty() && prepared.current.is_empty());
    assert_eq!(&*held.manifest, snapshot);
    assert!(!Arc::ptr_eq(&held.manifest, &prepared.manifest));
}

#[test]
fn preparation_scratch_trims_a_peak_and_recovers_after_rejection() {
    let large = frame(&(1..=200).map(|id| (id, 3)).collect::<Vec<_>>());
    let small = frame(&[(1, 3)]);
    let mut cache = BoneResidency::default();
    let prepared = prepare(&mut cache, &large);
    cache.recycle(prepared.previous, prepared.current);
    let peak = cache.scratch.assignments.capacity();
    for _ in 0..render_model::CapacityRetention::SHRINK_AFTER {
        let prepared = prepare(&mut cache, &small);
        cache.recycle(prepared.previous, prepared.current);
    }
    assert!(cache.scratch.assignments.capacity() < peak);
    assert!(cache.scratch.assignments.capacity() <= 4);
    assert!(
        cache
            .prepare(&ActorRigRenderFrame::default(), |_| {})
            .is_none()
    );
    assert_eq!(
        prepare(&mut cache, &small).uploaded_bytes(),
        6 * ACTOR_BONE_MATRIX_BYTES
    );
}
