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
