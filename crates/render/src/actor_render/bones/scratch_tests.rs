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

#[test]
fn unused_render_frames_release_peak_scratch_and_publications() {
    use super::super::tests::{app_with_noop_render_sub_app, generic_frame};
    use super::super::{ActorGpu, ActorRenderPlugin, prepare_actor_resources};
    use bevy::{
        ecs::system::RunSystemOnce,
        render::{RenderApp, RenderStartup},
    };
    use render_model::CapacityRetention;

    for rejected in [false, true] {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ActorRenderPlugin);
        app.finish();
        let world = app.sub_app_mut(RenderApp).world_mut();
        world.run_schedule(RenderStartup);
        let mut populated = generic_frame();
        let mut rig = frame(&(1..=200).map(|id| (id, 3)).collect::<Vec<_>>());
        rig.geometry_vertices = populated.rig.geometry_vertices.clone();
        populated.rig = rig;
        populated.instance_pages = vec![1; populated.rig.instances.len()].into();
        world.insert_resource(populated.clone());
        world.run_system_once(prepare_actor_resources).unwrap();
        let (instances, manifest, peak) = {
            let gpu = world.resource::<ActorGpu>();
            assert_eq!(gpu.instance_count as usize, populated.rig.instances.len());
            let mut receipts = gpu.receipt_scratch.lock().unwrap();
            receipts.extend_from_slice(&gpu.spans);
            receipts.record_usage();
            (
                Arc::downgrade(&gpu.instances),
                Arc::downgrade(&gpu.manifest),
                gpu.bones.scratch.assignments.capacity(),
            )
        };
        let mut idle = populated.clone();
        idle.rig.frame_generation += 1;
        if rejected {
            idle.rig.geometry_revision += 1;
            idle.rig.geometry_spans = Arc::from([]);
        } else {
            idle.rig.instances = Arc::from([]);
            idle.rig.manifest = Arc::from([]);
            idle.rig.previous_bones = Arc::from([]);
            idle.rig.current_bones = Arc::from([]);
            idle.instance_pages = Arc::from([]);
        }
        world.insert_resource(idle);
        // Each entry accounts for the completed prior frame, including its last populated use.
        let retired_at = CapacityRetention::SHRINK_AFTER + 1;
        for frame in 1..=retired_at {
            world.run_system_once(prepare_actor_resources).unwrap();
            let gpu = world.resource::<ActorGpu>();
            assert_eq!(gpu.instance_count, 0);
            assert_eq!(
                gpu.bones.scratch.assignments.capacity(),
                if frame == retired_at { 0 } else { peak }
            );
        }
        let gpu = world.resource::<ActorGpu>();
        assert_eq!(gpu.receipt_scratch.lock().unwrap().capacity(), 0);
        assert_eq!(gpu.bones.scratch.keys.capacity(), 0);
        assert!(instances.upgrade().is_none());
        assert!(manifest.upgrade().is_none());
        populated.rig.frame_generation += 2;
        world.insert_resource(populated);
        world.run_system_once(prepare_actor_resources).unwrap();
        assert_eq!(world.resource::<ActorGpu>().instance_count, 200);
    }
}
