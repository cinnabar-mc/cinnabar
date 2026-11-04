//! Readiness and final preparation share sampled native output.
use super::*;
use crate::actor_publication::{
    ActorFrameInput,
    bodies::PoseView,
    hand_frame::{self, HandCaches},
};

#[test]
fn readiness_and_prepare_reuse_sampling_without_allocating() {
    for java_mode in [false, true] {
        let (stream, mut equipment, artwork, input) = fixture(None);
        let mut cache = java::HandCache::default();
        let mut ready = Some(hand_source_for_mode(
            &stream,
            &mut equipment,
            &artwork,
            &input,
            &mut cache,
            0.5,
            java_mode,
        ));
        let canonical = Some(ready.as_ref().unwrap().presentation.clone());
        let animation = Some(actor_animation::AttachableAnimationInput {
            first_person: true,
            ..Default::default()
        });
        let key = source_key(&stream, None, animation, 0.5);
        let frame_input = ActorFrameInput {
            cosmetics: Vec::new(),
            local_feed: None,
            predicted_eye: None,
            predicted_feet: None,
            local_equipment: input.clone(),
            swing_progress: None,
            renders_game: true,
            hide_hand: false,
            remote_emotes: Vec::new(),
            remote_hats: Vec::new(),
            custom_emote: None,
        };
        let view = PoseView {
            partial_tick: 0.5,
            first_person: true,
            java_mode,
            combat_visuals: render_api::CombatVisuals::Modern,
            sampling_camera: Some(([0.; 2], [0.; 3])),
            aspect: 1.,
        };
        let work = cache.native_pose.sample_work;
        let samples = cache.native_pose.layer_samples;
        assert_eq!(samples, 1);
        let allocations = test_alloc::thread_allocations();
        let repeated = source(
            HandInputs {
                stream: &stream,
                presentation: canonical.as_ref().unwrap().clone(),
                equipment_input: &input,
                owner_equipment: &input,
                consume_ticks: None,
                item_animation: animation,
                alpha: 0.5,
                artwork: &artwork,
                motion: Mat4::IDENTITY,
                sampling_camera: view.sampling_camera,
                aspect: 1.,
            },
            java_mode,
            &mut equipment,
            &mut cache,
        )
        .unwrap();
        assert_eq!(
            test_alloc::thread_allocations(),
            allocations,
            "warm source must not allocate a visibility sample"
        );
        let prepared = hand_frame::prepare(
            Some(&stream),
            1,
            &canonical,
            &frame_input,
            view,
            (None, animation),
            &artwork,
            None,
            HandCaches {
                source: &mut ready,
                key: &key,
                java_hand: &mut cache,
                equipment: Some(&mut equipment),
            },
        )
        .unwrap();
        assert_eq!(
            test_alloc::thread_allocations(),
            allocations,
            "ready prepare must not allocate a visibility sample"
        );
        assert_eq!(cache.native_pose.sample_work, work);
        assert_eq!(cache.native_pose.layer_samples, samples);
        assert_eq!(prepared.visibility, repeated.visibility);
        assert!(ready.is_none(), "the ready source is reused");
    }
}

#[test]
fn readiness_visibility_and_native_pose_share_one_layer_sample() {
    let (stream, mut equipment, artwork, input) = fixture(Some("minecraft:shield"));
    let mut cache = java::HandCache::default();
    hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        0.5,
        false,
    );
    assert_eq!(cache.native_pose.layer_samples, 1);
    assert_eq!(
        cache.native_pose.sample_work.0, 1,
        "the native pose consumed that same sample"
    );
}

#[test]
fn advancing_frame_inputs_reuse_nonempty_layer_storage() {
    for (java_mode, main) in [(false, None), (true, None), (false, Some("minecraft:bow"))] {
        let ((mut stream, mut equipment, artwork, input), _) = fixture_with_visibility(
            main,
            false,
            false,
            true,
            false,
            false,
            Some("query.frame_alpha > 0.5"),
        );
        let mut cache = java::HandCache::default();
        let initial = hand_source_for_mode(
            &stream,
            &mut equipment,
            &artwork,
            &input,
            &mut cache,
            0.25,
            java_mode,
        );
        let presentation = initial.presentation;
        assert!(!stream.authority().actor_rig(1).unwrap().render.is_empty());
        for frame in 0..24 {
            let alpha = [0.0, 0.25, 0.75, 0.75][frame % 4];
            let camera = (
                [frame as f32, frame as f32 * 2.0],
                [frame as f32 * 0.1, 64.0, 0.0],
            );
            stream.set_actor_camera_rotation(camera.0);
            stream.set_actor_camera_position(camera.1);
            let allocations = test_alloc::thread_allocations();
            let selected = source(
                HandInputs {
                    stream: &stream,
                    presentation: presentation.clone(),
                    equipment_input: &input,
                    owner_equipment: &input,
                    consume_ticks: None,
                    item_animation: Some(actor_animation::AttachableAnimationInput {
                        first_person: true,
                        ..Default::default()
                    }),
                    alpha,
                    artwork: &artwork,
                    motion: Mat4::IDENTITY,
                    sampling_camera: Some(camera),
                    aspect: 1.0,
                },
                java_mode,
                &mut equipment,
                &mut cache,
            )
            .unwrap();
            if frame >= 4 {
                assert_eq!(
                    test_alloc::thread_allocations(),
                    allocations,
                    "frame {frame}, Java {java_mode}"
                );
            }
            assert_eq!(selected.visibility.hides_held_items(), alpha > 0.5);
        }
        if java_mode {
            assert_eq!(
                cache.native_pose.sample_work.0, 0,
                "Java visibility needs no native pose"
            );
        }
    }
}

#[test]
fn java_native_fallback_reuses_its_full_sample_for_later_visibility_reads() {
    let (stream, mut equipment, artwork, input) = fixture(Some("minecraft:shield"));
    let mut cache = java::HandCache::default();
    hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        0.5,
        true,
    );
    let samples = cache.native_pose.layer_samples;
    let work = cache.native_pose.sample_work;
    assert_eq!(work.0, 1);
    hand_source_for_mode(
        &stream,
        &mut equipment,
        &artwork,
        &input,
        &mut cache,
        0.5,
        true,
    );
    assert_eq!(cache.native_pose.layer_samples, samples);
    assert_eq!(cache.native_pose.sample_work, work);
}
