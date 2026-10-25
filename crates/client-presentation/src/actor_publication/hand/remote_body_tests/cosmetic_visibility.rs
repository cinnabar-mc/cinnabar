use super::*;
use crate::actor_publication::{ActorFrameInput, GroundEffectWearers, RemoteHat, hats::HatState};

#[test]
fn hiding_other_player_cosmetics_preserves_local_items_and_restores_remote_effects() {
    let ((mut stream, _, artwork, local_equipment), _) =
        super::super::native_tests::fixture_with_rotation(true);
    spawn_remotes(&mut stream);
    let glowing = render_model::hats::fixture::glowing_crown();
    let ground = render_model::hats::fixture::ground_effect();
    let revision = stream.authority().actor(2).unwrap().spawn_revision;
    let mut hats = HatState::default();
    let mut scene = render_scene::ActorRenderScene::default();
    let mut wearers = GroundEffectWearers::default();
    let mut emitters = Vec::new();

    for visible in [true, false, true] {
        let mut input = ActorFrameInput {
            cosmetics: vec![glowing, ground],
            remote_hats: [glowing, ground]
                .map(|hat| RemoteHat {
                    runtime_id: 2,
                    spawn_revision: revision,
                    hat,
                })
                .into(),
            local_feed: None,
            predicted_eye: None,
            predicted_feet: None,
            local_equipment: local_equipment.clone(),
            swing_progress: None,
            renders_game: true,
            hide_hand: false,
            custom_emote: None,
            remote_emotes: Vec::new(),
        };
        input.apply_cosmetic_visibility(visible);

        let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
        let mut batch = crate::presentation::actors::select_actor_presentations(
            1,
            true,
            bodies.canonical_local,
            bodies.remotes,
        );
        wearers.capture(&input, Some(&stream), &batch, true);
        assert!(wearers.0.iter().any(|wearer| wearer.runtime_id == 1));
        assert_eq!(
            wearers.0.iter().any(|wearer| wearer.runtime_id == 2),
            visible
        );

        hats.advance(&input.cosmetics, 0.0, 0.05);
        hats.attach(
            &mut batch,
            Some(&stream),
            &input.remote_hats,
            &mut scene,
            0.5,
            Default::default(),
        );
        let is_crown = |draw: &&render_model::ActorRigSubmission| {
            draw.input.rig == render_model::hats::rig(glowing).geometry.id
        };
        let crowns: Vec<_> = batch.submissions.iter().filter(is_crown).collect();
        assert!(
            crowns
                .iter()
                .any(|draw| draw.input.identity.runtime_id == 1)
        );
        assert_eq!(
            crowns
                .iter()
                .any(|draw| draw.input.identity.runtime_id == 2),
            visible
        );
        assert!(batch.submissions.iter().any(|draw| {
            draw.input.identity.runtime_id == 2
                && draw.input.identity.layer == render_model::ACTOR_LAYER_BODY
        }));
        hats.place_emitters(&batch, 0.5, &mut emitters);
        assert!(emitters.iter().any(|emitter| emitter.key.runtime_id == 1));
        assert_eq!(
            emitters.iter().any(|emitter| emitter.key.runtime_id == 2),
            visible
        );
    }
}

#[test]
fn first_person_local_cosmetics_schedule_no_attachment_emitter_or_ground_work() {
    let ((stream, _, artwork, equipment), _) =
        super::super::native_tests::fixture_with_rotation(true);
    let bodies = sample_shared_bodies(&stream, &artwork, &equipment, true, Vec::new());
    let body = bodies.canonical_local.unwrap().submission;
    let items = [
        render_model::hats::fixture::glowing_crown(),
        render_model::hats::fixture::cloth(),
        render_model::hats::fixture::ground_effect(),
    ];
    let mut hats = HatState::default();
    let mut scene = render_scene::ActorRenderScene::default();
    let mut batch = crate::presentation::actors::ActorPresentationBatch {
        submissions: Vec::new(),
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };
    let mut emitters = Vec::new();
    let mut wearers = GroundEffectWearers::default();
    let input = ActorFrameInput {
        cosmetics: items.to_vec(),
        remote_hats: Vec::new(),
        local_feed: None,
        predicted_eye: None,
        predicted_feet: None,
        local_equipment: equipment,
        swing_progress: None,
        renders_game: true,
        hide_hand: false,
        custom_emote: None,
        remote_emotes: Vec::new(),
    };
    hats.advance(&items, 6.0, 0.05);
    let (_, allocations) = test_alloc::count(|| {
        hats.attach(
            &mut batch,
            Some(&stream),
            &[],
            &mut scene,
            0.5,
            crate::actor_publication::hats::AttachmentFrame {
                hidden_local: Some(&body),
                ..Default::default()
            },
        );
        hats.place_emitters(&batch, 0.5, &mut emitters);
        wearers.capture(&input, Some(&stream), &batch, false);
    });
    assert_eq!(allocations, 0);
    assert!(batch.submissions.is_empty());
    assert!(batch.skin_layers.is_empty());
    assert!(emitters.is_empty());
    assert!(wearers.0.is_empty());
    for item in items {
        assert!(!scene.contains_geometry(render_model::hats::rig(item).geometry.id));
    }
}

#[test]
fn visible_static_cosmetics_reuse_poses_textures_and_submission_storage() {
    let ((stream, _, artwork, equipment), _) =
        super::super::native_tests::fixture_with_rotation(true);
    let bodies = sample_shared_bodies(&stream, &artwork, &equipment, false, Vec::new());
    let mut batch = crate::presentation::actors::select_actor_presentations(
        1,
        true,
        bodies.canonical_local,
        Vec::new(),
    );
    let body_count = batch.submissions.len();
    let skin_count = batch.skin_layers.len();
    let item = render_model::hats::fixture::glowing_crown();
    let mut hats = HatState::default();
    let mut scene = render_scene::ActorRenderScene::default();
    let mut emitters = Vec::new();
    for _ in 0..2 {
        batch.submissions.truncate(body_count);
        batch.skin_layers.truncate(skin_count);
        hats.advance(&[item], 0.0, 0.05);
        hats.attach(
            &mut batch,
            Some(&stream),
            &[],
            &mut scene,
            0.5,
            Default::default(),
        );
        hats.place_emitters(&batch, 0.5, &mut emitters);
    }
    let before = batch.submissions.last().unwrap().clone();
    let pixels = batch.skin_layers.last().unwrap().clone();
    let (_, allocations) = test_alloc::count(|| {
        batch.submissions.truncate(body_count);
        batch.skin_layers.truncate(skin_count);
        hats.advance(&[item], 0.0, 0.05);
        hats.attach(
            &mut batch,
            Some(&stream),
            &[],
            &mut scene,
            0.5,
            Default::default(),
        );
        hats.place_emitters(&batch, 0.5, &mut emitters);
    });
    assert_eq!(allocations, 0);
    let after = batch.submissions.last().unwrap();
    assert_eq!(before, *after, "unchanged GPU inputs need no new upload");
    assert!(std::sync::Arc::ptr_eq(
        &before.input.current_bones,
        &after.input.current_bones
    ));
    assert_eq!(pixels, *batch.skin_layers.last().unwrap());
    assert!(!emitters.is_empty());
}
