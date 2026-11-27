use super::*;
use render_model::{ActorArtworkPages, ActorGpuInstance, ActorRigRenderFrame, EquipmentRaster};
use std::sync::Arc;

/// An independent camera-space model with distinct endpoint matrices and retained artwork.
fn frame() -> HandRigFrame {
    let (pages, locations) =
        ActorArtworkPages::default().with_equipment_rasters(&[EquipmentRaster {
            width: 2,
            height: 2,
            rgba8: vec![255; 16].into(),
        }]);
    HandRigFrame {
        rig: ActorRigRenderFrame {
            frame_generation: 17,
            instances: Arc::from([
                ActorGpuInstance {
                    geometry_id: 3,
                    previous_bone_base: 0,
                    current_bone_base: 0,
                    world_from_actor: [[1., 0., 0., 2.], [0., 1., 0., 3.], [0., 0., 1., -4.]],
                    partial_tick: 0.5,
                    ..Default::default()
                },
                ActorGpuInstance {
                    geometry_id: 3,
                    previous_bone_base: 0,
                    current_bone_base: 0,
                    texture_layer: render_model::HAND_ITEM_LAYER_FLAG,
                    ..Default::default()
                },
            ]),
            previous_bones: Arc::from([[[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]]]),
            current_bones: Arc::from([[[1., 0., 0., 5.], [0., 1., 0., 0.], [0., 0., 1., 0.]]]),
            manifest: Arc::from([0, 1].map(|index| render_model::ActorDrawManifestEntry {
                identity: render_model::ActorRenderIdentity {
                    session_id: 1,
                    dimension: 0,
                    runtime_id: 2,
                    spawn_revision: 1.try_into().unwrap(),
                    ingress_sequence: 3,
                    source_tick: Some(4),
                    movement_revision: 5,
                    pose_generation: Default::default(),
                    layer: 0,
                },
                rig: render_model::EntityRigId(42),
                completed_tick: 4,
                reset_generation: 0,
                route: render_model::ActorRigRoute::Compiled,
                instance_index: index,
                previous_bone_base: 0,
                current_bone_base: 0,
                bone_count: 1,
            })),
            ..Default::default()
        },
        artwork: Some(Arc::new(pages)),
        draw_locations: Arc::from([locations[0], None]),
        skin: vec![0; render_model::STANDARD_SKIN_BYTES].into(),
        light: Default::default(),
        fov_radians: 1.2,
        revision: 9,
        item_atlases: [None, None, None],
        item_glint: None,
    }
}

#[test]
fn published_draws_preserve_order_artwork_and_both_pose_endpoints() {
    let frame = frame();
    let report = snapshot(&frame);
    assert_eq!(report["main_item_instances"], 1);
    assert_eq!(report["total_instances"], 2);
    let draws = report["draws"]
        .as_array()
        .expect("published draws are observable");
    assert_eq!(draws.len(), 2);
    assert_eq!(draws[0]["geometry_id"], 3);
    assert_eq!(
        draws[1]["geometry_id"], 3,
        "repeated geometry must not deduplicate draws"
    );
    assert_eq!(draws[0]["instance_index"], 0);
    assert_eq!(draws[1]["instance_index"], 1);
    assert_eq!(draws[0]["rig"], 42);
    assert_eq!(
        draws[0]["artwork_location"]["page"],
        frame.draw_locations[0].unwrap().page
    );
    assert_eq!(draws[0]["artwork_location"]["dimensions"], json!([2, 2]));
    assert_eq!(draws[0]["artwork_location"]["valid_layer"], true);
    assert!(draws[1]["artwork_location"].is_null());
    assert_eq!(draws[0]["previous_bones"][0][0][3], 0.0);
    assert_eq!(draws[0]["current_bones"][0][0][3], 5.0);
    assert_eq!(draws[0]["world_from_actor"][2][3], -4.0);
    assert_eq!(draws[0]["partial_tick"], 0.5);
    assert_eq!(report["omitted_instances"], 0);
}

#[test]
fn snapshot_bounds_draws_and_pose_output_and_reports_invalid_references() {
    let mut frame = frame();
    let instance = frame.rig.instances[0];
    frame.rig.instances = vec![instance; MAX_DRAWS + 1].into();
    let mut entry = frame.rig.manifest[0].clone();
    entry.bone_count = MAX_BONE_MATRICES as u32;
    frame.rig.manifest = Arc::from([entry]);
    frame.draw_locations = Arc::from([]);
    let report = snapshot(&frame);
    assert_eq!(report["listed_instances"], MAX_DRAWS);
    assert_eq!(report["omitted_instances"], 1);
    assert_eq!(report["bone_matrices"], 0);
    assert_eq!(report["bone_matrix_budget_used"], MAX_BONE_MATRICES);
    let draw = &report["draws"][0];
    assert_eq!(draw["listed_bones"], 0);
    assert_eq!(draw["omitted_bones"], MAX_BONE_MATRICES);
    assert_eq!(draw["previous_bone_range_valid"], false);
    assert_eq!(draw["current_bone_range_valid"], false);
    assert!(
        draw["previous_bones"].is_null(),
        "an invalid arena range is not an empty valid pose"
    );
    assert_eq!(draw["location_entry_present"], false);
    assert!(
        report["draws"][1]["rig"].is_null(),
        "missing manifest entries stay unknown"
    );
}

#[test]
fn counts_keep_legacy_main_offhand_and_skin_semantics() {
    let item = render_model::HAND_ITEM_LAYER_FLAG;
    let offhand = render_model::HAND_OFFHAND_LAYER_FLAG;
    let instances =
        [0, item, item | 1, item | offhand, offhand].map(|texture_layer| ActorGpuInstance {
            texture_layer,
            ..Default::default()
        });
    assert_eq!(item_counts(&instances), [2, 1]);
    assert_eq!(item_counts(&[]), [0, 0]);
}
