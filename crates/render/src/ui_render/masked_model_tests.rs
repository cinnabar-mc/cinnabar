use crate::render_test_support::scene_raster::{
    FixturePipelines, SIDE, app_with_effects, fixture_pipeline, frame, read_pixels, renderer,
};
use bevy::{
    color::Srgba,
    render::{RenderApp, renderer::RenderDevice},
};
use render_model::{
    UI_BLEND_ALPHA, UiModelBone, UiModelPose, UiRenderBatch, UiRenderInput, UiRenderVertex,
    UiScissor, UiTextureCatalog, UiTexturePage,
    actor::fixture::{MASK_TEXTURE_RGBA, MASK_TEXTURE_SIZE, mask_screen, masked_parts},
};
use render_scene::{UiRenderSceneResource, UiRenderStatsResource};
use std::sync::Arc;

#[test]
fn transparent_model_parts_occlude_later_color_without_painting_the_background() {
    let Some(render) = renderer() else { return };
    let (mut app, _, image) = app_with_effects(render, |app| {
        app.add_plugins(super::UiRenderPlugin);
    });
    let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    let fixture = FixturePipelines {
        checker: fixture_pipeline(device, 1, "checker"),
        sloped_checker: fixture_pipeline(device, 1, "sloped_checker"),
        silhouette: fixture_pipeline(device, 1, "silhouette"),
        scene: 0,
    };
    app.sub_app_mut(RenderApp).insert_resource(fixture);
    for _ in 0..8 {
        frame(&mut app);
    }
    let background = read_pixels(&app, &image);
    let model = masked_parts();
    let vertices = model
        .vertices
        .iter()
        .map(|vertex| UiRenderVertex {
            position: [vertex.position[0], vertex.position[1]],
            clip_z: vertex.position[2],
            clip_w: (vertex.bone_index + 1) as f32,
            uv: vertex.uv,
            color: [255; 4],
            style_flags: render_api::UI_STYLE_MODEL_VERTEX,
            alpha_cutoff: -1.,
            model_light: 1.,
            overlay_color: [0.; 4],
        })
        .collect::<Vec<_>>();
    let count = vertices.len() as u32;
    let bone = UiModelBone {
        screen: mask_screen(),
        normal: [[0.; 4]; 3],
        overlay: [0.; 4],
        uv_rect: [
            0.,
            0.,
            MASK_TEXTURE_SIZE[0] as f32,
            MASK_TEXTURE_SIZE[1] as f32,
        ],
        uv_transform: [0., 0., 1., 1.],
        uv_cross: [0.; 4],
        tint: [1.; 4],
        styles: [0, 1, 0, 0],
    };
    let mut input = UiRenderInput {
        revision: 1,
        viewport_size: [SIDE; 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: (0..count).collect(),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, SIDE, SIDE),
            0,
            count,
            UI_BLEND_ALPHA,
        )
        .with_depth_test(true)
        .with_depth_write(true)
        .with_isolated_depth_scope(Some(1))]),
        models: Arc::from([UiModelPose {
            scope: 1,
            bones: Arc::from([bone; 2]),
            bounds: UiScissor::new(0, 0, SIDE, SIDE),
        }]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![
                    UiTexturePage::owned(MASK_TEXTURE_SIZE, MASK_TEXTURE_RGBA.to_vec().into())
                        .unwrap(),
                ],
                1,
            )
            .unwrap(),
        ),
    };
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    let at = |bytes: &[u8], x: usize| bytes[(32 * SIDE as usize + x) * 4..][..4].to_vec();
    let mut centers = std::collections::BTreeSet::new();
    let mut artwork = Vec::new();
    for (index, phase) in [0., 0.25, 0.5, 0.75].into_iter().enumerate() {
        input.revision = index as u64 + 1;
        let mut animated = bone;
        animated.uv_transform[0] = phase;
        Arc::make_mut(&mut input.models)[0].bones = Arc::from([animated; 2]);
        app.world_mut()
            .resource_mut::<UiRenderSceneResource>()
            .publish(input.clone(), &stats)
            .unwrap();
        for _ in 0..8 {
            frame(&mut app);
        }
        let pixels = read_pixels(&app, &image);
        assert_eq!(
            at(&pixels, 12),
            at(&background, 12),
            "the left mask writes depth but no color at phase {phase}"
        );
        assert_eq!(
            at(&pixels, 52),
            at(&background, 52),
            "the right mask writes depth but no color at phase {phase}"
        );
        let center = at(&pixels, 32);
        assert_ne!(
            center,
            at(&background, 32),
            "the opening retains artwork at phase {phase}"
        );
        assert!(
            centers.insert(center.clone()),
            "each quarter phase samples distinct source artwork"
        );
        artwork.push(center);
    }
    for (index, phase) in [0., 0.25, 0.5, 0.75].into_iter().enumerate() {
        input.revision += 1;
        let mut animated = bone;
        animated.uv_transform[0] = phase;
        animated.overlay = [0.8, 0.1, 0.2, 0.5];
        Arc::make_mut(&mut input.models)[0].bones = Arc::from([animated; 2]);
        app.world_mut()
            .resource_mut::<UiRenderSceneResource>()
            .publish(input.clone(), &stats)
            .unwrap();
        for _ in 0..8 {
            frame(&mut app);
        }
        let pixels = read_pixels(&app, &image);
        for x in [12, 52] {
            assert_eq!(
                at(&pixels, x),
                at(&background, x),
                "a tinted mask keeps zero alpha"
            );
        }
        let center = at(&pixels, 32);
        for channel in 0..3 {
            let source = Srgba::gamma_function_inverse(f32::from(artwork[index][channel]) / 255.);
            let mixed = source * (1. - animated.overlay[3])
                + animated.overlay[channel] * animated.overlay[3];
            let expected = (Srgba::gamma_function(mixed) * 255.).round() as i16;
            assert!(
                (i16::from(center[channel]) - expected).abs() <= 1,
                "controller overlay survives repeated UVs at phase {phase}, channel {channel}: {center:?}"
            );
        }
        assert_eq!(
            center[3], artwork[index][3],
            "overlay does not change source alpha"
        );
    }
}
