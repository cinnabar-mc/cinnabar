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
        wearers.capture(&input, Some(&stream));
        assert!(wearers.0.iter().any(|wearer| wearer.runtime_id == 1));
        assert_eq!(
            wearers.0.iter().any(|wearer| wearer.runtime_id == 2),
            visible
        );

        let bodies = sample_shared_bodies(&stream, &artwork, &local_equipment, false, Vec::new());
        let mut batch = crate::presentation::actors::select_actor_presentations(
            1,
            true,
            bodies.canonical_local,
            bodies.remotes,
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
