//! Geometry failures must reject admission without losing the next publication's uploads.

use super::{resources::prepare_ui_resources, *};
use bevy::ecs::system::RunSystemOnce;
use render_model::{
    UI_DYNAMIC_PAGE_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET, UiScissor, UiTextureCatalog, UiTexturePage,
};
use std::{cell::Cell, rc::Rc};

/// Builds a triangle on the resizable player-skin slot in a valid dynamic catalog.
fn input() -> UiRenderInput {
    let blank = UiTexturePage::owned(
        [UI_DYNAMIC_PAGE_SIDE; 2],
        vec![255; (UI_DYNAMIC_PAGE_SIDE * UI_DYNAMIC_PAGE_SIDE * 4) as usize].into(),
    )
    .unwrap();
    UiRenderInput {
        models: Default::default(),
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: vec![
            UiRenderVertex {
                position: [1.0; 2],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            };
            3
        ]
        .into(),
        indices: Arc::from([0, 1, 2]),
        batches: Arc::from([UiRenderBatch::new(
            UI_PLAYER_SKIN_PAGE_OFFSET as u32,
            UiScissor::new(0, 0, 64, 64),
            0,
            3,
            0,
        )]),
        textures: Arc::new(
            UiTextureCatalog::new(vec![blank; UI_PLAYER_SKIN_PAGE_OFFSET + 1], 0).unwrap(),
        ),
    }
}

/// Publishes through the scene owner before running the actual GPU preparation system.
fn publish(world: &mut World, input: UiRenderInput) {
    let stats = world.resource::<UiRenderStatsResource>().clone();
    world
        .resource_mut::<UiRenderSceneResource>()
        .publish(input, &stats)
        .unwrap();
}

#[test]
fn geometry_oom_after_texture_resize_rejects_and_recovers_publication() {
    for failed_stage in [
        "shared bounded UI vertex arena",
        "shared bounded UI index arena",
        "UI vertex upload",
        "UI index upload",
    ] {
        for revert in [false, true] {
            let mut world = ordered_command_tests::binding_world();
            let original = input();
            publish(&mut world, original.clone());
            world.run_system_once(prepare_ui_resources).unwrap();

            let mut resized = original.clone();
            resized.revision = 2;
            resized.vertices = vec![original.vertices[0]; 6].into();
            resized.indices = Arc::from([0, 1, 2, 3, 4, 5]);
            resized.batches = Arc::from([UiRenderBatch {
                index_count: 6,
                ..original.batches[0]
            }]);
            let mut pages = original.textures.pages().to_vec();
            let side = render_api::CLASSIC_SKIN_SIDE as u32;
            pages[UI_PLAYER_SKIN_PAGE_OFFSET] =
                UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into())
                    .unwrap();
            resized.textures = Arc::new(original.textures.replace_dynamic(pages).unwrap());
            publish(&mut world, resized);
            let reached = Rc::new(Cell::new(false));
            let observed = reached.clone();
            let failure = crate::gpu_allocation::inject(move |label, _| {
                if label == failed_stage {
                    observed.set(true);
                    true
                } else {
                    label == "bounded UI dimension bucket"
                }
            });
            world.run_system_once(prepare_ui_resources).unwrap();
            let gpu = world.resource::<UiGpu>();
            assert_eq!(gpu.accepted_revision, None, "{failed_stage}");
            assert!(gpu.batches.is_empty());
            assert!(gpu.animation_damage.is_empty());
            assert_eq!(gpu.last_admitted_revision, Some(1));
            assert!(
                reached.get(),
                "failure boundary not reached: {failed_stage}"
            );
            let stats = world.resource::<UiRenderStatsResource>().snapshot();
            assert_eq!(stats.rejected_revision, Some(2));
            assert!(stats.rejected_reason.is_some());
            assert_eq!(stats.draw_calls, 0);
            drop(failure);

            if revert {
                let mut restored = original;
                restored.revision = 3;
                publish(&mut world, restored);
            }
            // Geometry can recover immediately even while the texture plan is in backoff.
            world.run_system_once(prepare_ui_resources).unwrap();
            let stats = world.resource::<UiRenderStatsResource>().snapshot();
            assert_eq!(stats.accepted_revision, Some(if revert { 3 } else { 2 }));
            assert_eq!(stats.rejected_revision, None);
            assert_eq!(stats.uploaded_vertices, if revert { 3 } else { 6 });
            assert_eq!(stats.uploaded_indices, if revert { 3 } else { 6 });
            let writes = world.resource::<UiGpu>().geometry_writes;
            world.run_system_once(prepare_ui_resources).unwrap();
            assert_eq!(world.resource::<UiGpu>().geometry_writes, writes);
        }
    }
}

#[test]
fn geometry_upload_oom_retries_changed_bytes_in_retained_arenas() {
    for failed_stage in ["UI vertex upload", "UI index upload"] {
        let mut world = ordered_command_tests::binding_world();
        let original = input();
        publish(&mut world, original.clone());
        world.run_system_once(prepare_ui_resources).unwrap();
        let mut changed = original;
        changed.revision = 2;
        Arc::make_mut(&mut changed.vertices)[1].position[0] = 2.0;
        changed.indices = Arc::from([2, 1, 0]);
        publish(&mut world, changed);

        let failure = crate::gpu_allocation::inject(move |label, _| label == failed_stage);
        world.run_system_once(prepare_ui_resources).unwrap();
        assert_eq!(
            world.resource::<UiGpu>().accepted_revision,
            None,
            "{failed_stage}"
        );
        drop(failure);
        world.run_system_once(prepare_ui_resources).unwrap();
        let stats = world.resource::<UiRenderStatsResource>().snapshot();
        assert_eq!(stats.accepted_revision, Some(2));
        assert_eq!(stats.uploaded_vertices, 3);
        assert_eq!(stats.uploaded_indices, 3);
    }
}

/// Builds an admitted palette-addressed triangle on the ordinary player-skin page.
fn model_input() -> UiRenderInput {
    let mut frame = input();
    for vertex in Arc::make_mut(&mut frame.vertices) {
        vertex.style_flags |= render_api::UI_STYLE_MODEL_VERTEX;
        vertex.overlay_color = [0., 0., 1., 0.];
    }
    Arc::make_mut(&mut frame.batches)[0] = frame.batches[0]
        .with_depth_test(true)
        .with_depth_write(true)
        .with_isolated_depth_scope(Some(7));
    let bone = render_model::UiModelBone {
        screen: glam::Mat4::IDENTITY.to_cols_array_2d(),
        normal: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        overlay: [0.; 4],
        uv_rect: [0., 0., 1., 1.],
        uv_cross: [0.; 4],
        tint: [1.; 4],
        styles: [0; 4],
    };
    frame.models = Arc::from([render_model::UiModelPose {
        scope: 7,
        bones: Arc::from([bone]),
        bounds: UiScissor::new(0, 0, 64, 64),
    }]);
    frame
}

/// Animated model palettes never upload the resident mesh or replace its shared arenas.
#[test]
fn animated_ui_model_updates_only_the_bone_palette() {
    let mut world = ordered_command_tests::binding_world();
    let mut frame = model_input();
    let bone = frame.models[0].bones[0];
    publish(&mut world, frame.clone());
    world.run_system_once(prepare_ui_resources).unwrap();
    let gpu = world.resource::<UiGpu>();
    let writes = gpu.geometry_writes;
    let arenas = (
        gpu.vertex_arena_id,
        gpu.index_arena_id,
        gpu.models.buffer.id(),
    );
    for step in 2..6 {
        frame.revision = step;
        let mut moved = bone;
        moved.screen[3][0] = step as f32;
        frame.models = Arc::from([render_model::UiModelPose {
            scope: 7,
            bones: Arc::from([moved]),
            bounds: UiScissor::new(0, 0, 64, 64),
        }]);
        publish(&mut world, frame.clone());
        world.run_system_once(prepare_ui_resources).unwrap();
        let gpu = world.resource::<UiGpu>();
        assert_eq!(gpu.accepted_revision, Some(step));
        assert_eq!(
            gpu.geometry_writes, writes,
            "model animation uploaded static geometry"
        );
        assert_eq!(
            (
                gpu.vertex_arena_id,
                gpu.index_arena_id,
                gpu.models.buffer.id()
            ),
            arenas
        );
        let stats = world.resource::<UiRenderStatsResource>().snapshot();
        assert_eq!((stats.uploaded_vertices, stats.uploaded_indices), (0, 0));
        assert_eq!(stats.draw_calls, 1);
    }
}

/// Failed palette allocation or writes retain the last baseline and retry the same publication.
#[test]
fn model_palette_oom_retries_without_committing_or_replacing_resident_bindings() {
    for grow in [false, true] {
        for stage in ["retained UI model bone palette", "UI model palette upload"] {
            if !grow && stage == "retained UI model bone palette" {
                continue;
            }
            let mut world = ordered_command_tests::binding_world();
            let mut frame = model_input();
            publish(&mut world, frame.clone());
            world.run_system_once(prepare_ui_resources).unwrap();
            let buffer = world.resource::<UiGpu>().models.buffer.id();
            frame.revision += 1;
            let mut bone = frame.models[0].bones[0];
            bone.screen[3][0] += 5.;
            Arc::make_mut(&mut frame.models)[0].bones = vec![bone; if grow { 2 } else { 1 }].into();
            publish(&mut world, frame);
            let reached = Rc::new(Cell::new(false));
            let seen = reached.clone();
            let failure = crate::gpu_allocation::inject(move |label, _| {
                if label == stage {
                    seen.set(true);
                    true
                } else {
                    false
                }
            });
            world.run_system_once(prepare_ui_resources).unwrap();
            let gpu = world.resource::<UiGpu>();
            assert!(reached.get(), "failure boundary not reached: {stage}");
            assert_eq!(gpu.accepted_revision, None);
            assert_eq!(gpu.last_admitted_revision, Some(1));
            assert!(gpu.batches.is_empty());
            assert_eq!(gpu.models.buffer.id(), buffer);
            drop(failure);
            world.run_system_once(prepare_ui_resources).unwrap();
            let gpu = world.resource::<UiGpu>();
            assert_eq!(gpu.accepted_revision, Some(2));
            assert_eq!(gpu.last_admitted_revision, Some(2));
            assert_eq!(gpu.models.buffer.id() != buffer, grow);
        }
    }
}

/// An interrupted in-place write must rewrite all bones, even when the next pose restores old data.
#[test]
fn failed_model_palette_upload_repairs_restored_and_partly_changed_poses() {
    for restore_original in [false, true] {
        let mut world = ordered_command_tests::binding_world();
        let mut original = model_input();
        let bone = original.models[0].bones[0];
        Arc::make_mut(&mut original.models)[0].bones = Arc::from([bone; 2]);
        publish(&mut world, original.clone());
        world.run_system_once(prepare_ui_resources).unwrap();
        let mut changed = original.clone();
        changed.revision = 2;
        Arc::make_mut(&mut Arc::make_mut(&mut changed.models)[0].bones)[0].screen[3][0] = 5.;
        publish(&mut world, changed);
        let failure = crate::gpu_allocation::inject(|label, _| label == "UI model palette upload");
        world.run_system_once(prepare_ui_resources).unwrap();
        assert_eq!(world.resource::<UiGpu>().accepted_revision, None);
        drop(failure);
        let mut repaired = original;
        repaired.revision = 3;
        if !restore_original {
            Arc::make_mut(&mut Arc::make_mut(&mut repaired.models)[0].bones)[1].screen[3][1] = 7.;
        }
        publish(&mut world, repaired);
        let uploaded = Rc::new(Cell::new(0));
        let observed = Rc::clone(&uploaded);
        let witness = crate::gpu_allocation::inject(move |label, bytes| {
            if label == "UI model palette upload" {
                observed.set(bytes);
            }
            false
        });
        world.run_system_once(prepare_ui_resources).unwrap();
        assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(3));
        assert_eq!(
            uploaded.get(),
            (2 * size_of::<render_model::UiModelBone>()) as u64
        );
        drop(witness);
    }
}
