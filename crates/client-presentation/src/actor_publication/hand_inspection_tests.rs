use super::*;
use render_model::*;
use std::sync::Arc;

fn draw(rig: u32) -> ActorRigSubmission {
    let bones: Arc<[_]> = vec![
        RenderBoneTransform {
            rotation: [0., 0., 0., 1.],
            translation_scale: [0., 0., 0., 1.],
            axis_scale: [1.; 4],
        };
        diagnostic_geometry().bone_pivots.len()
    ]
    .into();
    ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: std::num::NonZeroU64::MIN,
                ingress_sequence: 3,
                source_tick: None,
                movement_revision: 4,
                pose_generation: render_api::PoseGeneration::new(5),
                layer: 0,
            },
            rig: EntityRigId(rig),
            previous_bones: bones.clone(),
            current_bones: bones,
            completed_tick: 7,
            reset_generation: 1,
        },
        world_from_actor: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        overlay_rgba8: 0,
        uv_anim: IDENTITY_UV_ANIM,
        light: 0,
    }
}

#[test]
fn inspection_pairs_missing_and_repeated_admitted_draws_with_one_real_build() {
    let g = diagnostic_geometry().with_id(EntityRigId(11));
    let mut builder = ActorRigFrameBuilder::new([g.clone()]).unwrap();
    let mut world = ActorRenderScene::default();
    world
        .insert_geometries(vec![g, diagnostic_geometry().with_id(EntityRigId(12))])
        .unwrap();
    let draws = vec![
        ("controller", draw(12), None),
        ("main_item", draw(11), None),
        ("controller", draw(11), None),
    ];
    let mut inspector = HandInspection::default();
    assert!(!inspector.take_request());
    assert!(inspector.snapshot().is_none());
    inspector.request();
    assert!(inspector.take_request());
    assert!(!inspector.take_request());
    let before = HandInspection::before(&builder, &world, None, draws.clone());
    assert_eq!(before["total_submissions"], 3);
    assert_eq!(before["submissions"][0]["category"], "controller");
    assert_eq!(before["submissions"][0]["requested_rig"], 12);
    assert_eq!(before["submissions"][0]["hand_registered_before"], false);
    assert_eq!(before["submissions"][0]["world_registered_before"], true);
    let rig = builder.build_ordered_located(
        0.5,
        None,
        draws.into_iter().map(|(_, d, l)| (d, l)),
        |_, _| 0,
    );
    assert_eq!(rig.rejects.missing_geometry, 1);
    let mut scene = HandRigScene::default();
    assert!(scene.publish(
        rig,
        vec![0; STANDARD_SKIN_BYTES].into(),
        Default::default(),
        1.2,
        9
    ));
    inspector.finish(before, &builder, &scene);
    let report = inspector.snapshot().unwrap();
    assert!(report["submissions"][0]["published_instance_index"].is_null());
    assert_eq!(
        report["submissions"][1]["publication_match_ambiguous"],
        true
    );
    assert_eq!(
        report["submissions"][1]["matching_instance_indices"],
        json!([0, 1])
    );
    assert!(report["submissions"][1]["published_instance_index"].is_null());
    assert!(report["submissions"][2]["published_instance_index"].is_null());
    assert_eq!(
        report["published_frame_generation"],
        scene.frame.as_ref().unwrap().rig.frame_generation
    );
    assert_eq!(report["published_revision"], 9);
    inspector.request();
    inspector.clear_snapshot();
    assert!(inspector.snapshot().is_none());
    assert!(
        inspector.take_request(),
        "a session boundary must not lose a pending request"
    );
}

#[test]
fn rejected_duplicate_cannot_steal_a_later_draws_manifest_match() {
    let mut builder =
        ActorRigFrameBuilder::new([diagnostic_geometry().with_id(EntityRigId(11))]).unwrap();
    let world = ActorRenderScene::default();
    let mut invalid = draw(11);
    let mut pose = invalid.input.previous_bones.to_vec();
    pose[0].rotation[0] = f32::NAN;
    invalid.input.previous_bones = pose.into();
    let draws = vec![
        ("controller", invalid, None),
        ("controller", draw(11), None),
    ];
    let before = HandInspection::before(&builder, &world, None, draws.clone());
    let rig = builder.build_ordered_located(
        0.5,
        None,
        draws.into_iter().map(|(_, d, l)| (d, l)),
        |_, _| 0,
    );
    assert_eq!(rig.rejects.non_finite_pose, 1);
    let mut scene = HandRigScene::default();
    assert!(scene.publish(
        rig,
        vec![0; STANDARD_SKIN_BYTES].into(),
        Default::default(),
        1.2,
        1
    ));
    let mut inspector = HandInspection::default();
    inspector.finish(before, &builder, &scene);
    for row in inspector.snapshot().unwrap()["submissions"]
        .as_array()
        .unwrap()
    {
        assert_eq!(row["publication_match_ambiguous"], true);
        assert!(row["published_instance_index"].is_null());
        assert_eq!(row["matching_instance_indices"], json!([0]));
    }
}

#[test]
fn session_reset_clears_evidence_but_preserves_pending_request() {
    let builder = ActorRigFrameBuilder::new([]).unwrap();
    let scene = HandRigScene::default();
    let mut inspector = HandInspection::default();
    inspector.finish(json!({"submissions": []}), &builder, &scene);
    inspector.request();
    inspector.clear_snapshot();
    assert!(inspector.snapshot().is_none());
    assert!(inspector.take_request());
}

#[test]
fn inspection_bounds_output_and_preserves_fully_rejected_frame_evidence() {
    let mut builder = ActorRigFrameBuilder::new([]).unwrap();
    let world = ActorRenderScene::default();
    let draws = vec![("controller", draw(12), None); MAX_SUBMISSIONS + 2];
    let before = HandInspection::before(&builder, &world, None, draws.clone());
    assert_eq!(before["total_submissions"], MAX_SUBMISSIONS + 2);
    assert_eq!(before["omitted_submissions"], 2);
    let rig = builder.build_ordered_located(
        1.,
        None,
        draws.into_iter().map(|(_, d, l)| (d, l)),
        |_, _| 0,
    );
    assert!(rig.instances.is_empty());
    let mut scene = HandRigScene::default();
    assert!(!scene.publish(
        rig,
        vec![0; STANDARD_SKIN_BYTES].into(),
        Default::default(),
        1.2,
        1
    ));
    let mut inspector = HandInspection::default();
    inspector.finish(before, &builder, &scene);
    assert!(inspector.snapshot().unwrap()["published_frame_generation"].is_null());
    assert_eq!(
        inspector.snapshot().unwrap()["total_submissions"],
        MAX_SUBMISSIONS + 2
    );
}
