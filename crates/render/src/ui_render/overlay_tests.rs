use super::*;

#[test]
fn retained_hand_split_keeps_mixed_bucket_layer_blend_and_shadow_fill_order() {
    let plan =
        render_model::UiTexturePlan::new(&[[1024, 1024], [2048, 2048], [256, 256], [2048, 2048]])
            .unwrap();
    let batches = [1, 0, 3, 2, 1]
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            UiRenderBatch::new(
                page,
                render_model::UiScissor::new(index as u32, 0, 20, 20),
                index as u32 * 18,
                18,
                if index == 2 {
                    UI_BLEND_INVERT
                } else {
                    UI_BLEND_ALPHA
                },
            )
        })
        .collect::<Vec<_>>();
    let trace = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
        .unwrap()
        .flat_map(|(index, batch, location)| {
            retained_batch_ranges(batch, Some(&(60..66)))
                .into_iter()
                .flatten()
                .map(move |range| {
                    (
                        index,
                        location.bucket,
                        location.layer,
                        batch.blend_mode,
                        batch.scissor.x,
                        range,
                    )
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        trace,
        vec![
            (0, 1, 0, UI_BLEND_ALPHA, 0, 0..18),
            (1, 0, 0, UI_BLEND_ALPHA, 1, 18..36),
            (2, 1, 1, UI_BLEND_INVERT, 2, 36..54),
            (3, 2, 0, UI_BLEND_ALPHA, 3, 54..60),
            (3, 2, 0, UI_BLEND_ALPHA, 3, 66..72),
            (4, 1, 0, UI_BLEND_ALPHA, 4, 72..90),
        ]
    );
    let full = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
        .unwrap()
        .flat_map(|(_, batch, _)| retained_batch_ranges(batch, None).into_iter().flatten())
        .collect::<Vec<_>>();
    assert_eq!(full, vec![0..18, 18..36, 36..54, 54..72, 72..90]);
    assert!(resolved_batches(None, &batches, plan.locations(), plan.buckets()).is_none());
    let mut locations = plan.locations().to_vec();
    locations[1].layer = 2;
    assert!(
        resolved_batches(Some(7), &batches, &locations, plan.buckets()).is_none(),
        "late invalid physical layer must emit no prefix"
    );
}

/// A depth-tested model whose depth surface failed to allocate keeps queueing on identical
/// frames, so the surface returns once memory frees instead of staying missing.
#[test]
fn identical_frames_retry_a_failed_model_depth_surface() {
    use bevy::render::{
        render_resource::{
            BufferDescriptor, BufferUsages, Extent3d, TextureDescriptor, TextureDimension,
            TextureUsages, TextureViewDescriptor,
        },
        view::{ExtractedView, RetainedViewEntity},
    };
    let mut world = super::super::ordered_command_tests::binding_world();
    let composite = super::super::composite::UiCompositePipeline::from_world(&mut world);
    world.insert_resource(composite);
    world.init_resource::<super::super::model_depth::UiModelDepths>();
    let device = world.resource::<RenderDevice>().clone();
    let buffer = |usage| {
        device.create_buffer(&BufferDescriptor {
            label: Some("test UI geometry"),
            size: 64,
            usage,
            mapped_at_creation: false,
        })
    };
    {
        let mut gpu = world.resource_mut::<UiGpu>();
        gpu.vertex_buffer = Some(buffer(BufferUsages::VERTEX));
        gpu.index_buffer = Some(buffer(BufferUsages::INDEX));
        gpu.batches = Arc::from([UiRenderBatch {
            depth_test: 1,
            isolated_depth_scope: Some(1),
            ..UiRenderBatch::new(
                0,
                render_model::UiScissor::new(0, 0, 8, 8),
                0,
                3,
                UI_BLEND_ALPHA,
            )
        }]);
    }
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("test UI layer"),
        size: Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: super::super::composite::UI_LAYER_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let layer_view = texture.create_view(&TextureViewDescriptor::default());
    let view = world.spawn_empty().id();
    world.entity_mut(view).insert((
        Msaa::Off,
        crate::render_test_support::camera(false),
        super::super::composite::UiLayerTexture::detached(texture, layer_view),
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(view.into(), None, 0),
            clip_from_view: bevy::math::Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: bevy::math::UVec4::new(0, 0, 8, 8),
            color_grading: Default::default(),
            invert_culling: false,
        },
    ));
    let queue = world.register_system(queue_ui_overlay);
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label == "JSON-UI model depth");
        world.run_system(queue).unwrap();
    }
    let has_depth = |world: &World| {
        world
            .resource::<super::super::model_depth::UiModelDepths>()
            .views
            .contains_key(&view)
    };
    assert!(!has_depth(&world));
    for _ in 0..8 {
        world.run_system(queue).unwrap();
    }
    assert!(
        has_depth(&world),
        "identical frames retry the model depth allocation"
    );
}
