use super::*;
use render_scene::world_material::WorldMaterialScene;

#[test]
fn bound_entities_tag_body_and_controller_layers_only() {
    let scene = WorldMaterialScene::with_bound_entities(&["example:other", "example:moth"]);
    let moth = protocol::ActorKind::Entity {
        identifier: "example:moth".into(),
    };
    assert_eq!(
        world(render_model::ACTOR_LAYER_BODY, &moth, &scene).map(|bound| bound.tag),
        Some(2)
    );
    assert_eq!(
        world(ACTOR_LAYER_TEXTURE_BASE, &moth, &scene).map(|bound| bound.tag),
        Some(2)
    );
    assert_eq!(
        world(render_model::ACTOR_LAYER_BODY + 1, &moth, &scene),
        None,
        "equipment keeps its own material"
    );
    let unbound = protocol::ActorKind::Entity {
        identifier: "example:pig".into(),
    };
    assert_eq!(
        world(render_model::ACTOR_LAYER_BODY, &unbound, &scene),
        None
    );
    let player = protocol::ActorKind::Player {
        uuid: [0; 16],
        username: "example:moth".into(),
    };
    assert_eq!(world(render_model::ACTOR_LAYER_BODY, &player, &scene), None);
}

#[test]
fn a_reaching_material_grows_the_culling_box_in_blocks_at_any_scale() {
    let submission = |scale: f32| render_model::ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: render_model::ActorRigRenderInput {
            identity: render_model::ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 7,
                spawn_revision: std::num::NonZeroU64::MIN,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: render_api::PoseGeneration::new(1),
                layer: render_model::ACTOR_LAYER_BODY,
            },
            rig: render_model::EntityRigId(1),
            previous_bones: std::sync::Arc::from([]),
            current_bones: std::sync::Arc::from([]),
            completed_tick: 1,
            reset_generation: 0,
        },
        world_from_actor: [
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
        ],
        texture_layer: 0,
        route: render_model::ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render_model::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let plain = assets::SkinGeometryBounds::default().half_extents;
    let mut grown = submission(1.0);
    grow_culling_bounds(&mut grown, 1.5);
    assert_eq!(grown.culling_bounds.half_extents, plain.map(|e| e + 1.5));
    // A half-size mob's model-space box grows twice as much, still 1.5 blocks in the world.
    let mut baby = submission(0.5);
    grow_culling_bounds(&mut baby, 1.5);
    assert_eq!(baby.culling_bounds.half_extents, plain.map(|e| e + 3.0));
    let mut still = submission(1.0);
    grow_culling_bounds(&mut still, 0.0);
    grow_culling_bounds(&mut still, f32::NAN);
    assert_eq!(still.culling_bounds.half_extents, plain);
}
