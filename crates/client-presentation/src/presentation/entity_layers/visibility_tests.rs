use super::*;
use actor_animation::{ActorRenderLayers, ControllerActivity, RenderVisibility};
use render_model::ActorRigRoute;

/// Places a pristine runtime skin before controllers select its render policy.
fn batch() -> ActorPresentationBatch {
    let presentation = crate::presentation::actors::local_diagnostic_presentation(
        1,
        0,
        1,
        render_api::PoseGeneration::new(1),
        [0.0; 3],
        0.0,
        0.0,
    )
    .unwrap();
    crate::presentation::actors::select_actor_presentations(1, true, Some(presentation), [])
}

#[test]
fn visibility_all_false_suppresses_body_without_hiding_equipment() {
    let mut cache = LayerPoseCache::default();
    for controllers in [
        None,
        Some(ControllerActivity::NotEvaluated),
        Some(ControllerActivity::NoControllers),
        Some(ControllerActivity::Active),
        Some(ControllerActivity::Inactive),
        Some(ControllerActivity::Active),
    ] {
        let mut batch = batch();
        let before = batch.submissions[0].route;
        for layer in [
            crate::presentation::equipment::LAYER_MAIN_HAND,
            crate::presentation::equipment::LAYER_OFF_HAND,
            crate::presentation::equipment::ELYTRA_LAYER,
        ] {
            let mut draw = batch.submissions[0].clone();
            draw.input.identity.layer = layer;
            batch.submissions.push(draw);
        }
        apply_render_layers_cached(
            &mut batch,
            |_| {
                controllers.map(|controllers| ActorRenderLayers {
                    render: Cow::Borrowed(&[]),
                    visibility: RenderVisibility {
                        controllers,
                        hide_held_items: Some(false),
                    },
                    skin: Cow::Borrowed(&[]),
                    held_item_scale: None,
                    head_rotation: None,
                })
            },
            &ActorArtworkPages::default(),
            &mut cache,
        );
        let hidden = controllers == Some(ControllerActivity::Inactive);
        assert_eq!(
            batch.submissions[0].route,
            if hidden {
                ActorRigRoute::NoDraw
            } else {
                before
            }
        );
        assert!(
            batch.submissions[1..]
                .iter()
                .all(|draw| draw.route == before)
        );
        if hidden {
            let mut skins = crate::presentation::skin_layers::ActorLayerCache::default();
            skins.apply(
                &mut batch,
                &ActorArtworkPages::default(),
                |_| panic!("hidden bodies must not publish skin attachments"),
                &mut crate::presentation::skin_rig::SkinRigCache::default(),
                |_| panic!("hidden bodies must not register skin geometry"),
                |_, _| None,
            );
            assert_eq!(batch.submissions.len(), 4);
        }
    }
}
