use crate::render_test_support::scene_raster::{
    FixturePipelines, SIDE, app_with_effects, fixture_pipeline, frame, read_pixels, renderer,
};
use bevy::render::{RenderApp, renderer::RenderDevice};
use render_model::{
    UiModelBone, UiModelPose, UiRenderBatch, UiRenderInput, UiRenderVertex, UiScissor,
    UiTextureCatalog, UiTexturePage,
    actor::fixture::{BOX_TEXTURE_SIZE, box_uv_geometry, box_uv_pixels, mask_screen},
};
use std::sync::Arc;

/// Builds the same scene for a box and its independently authored front-only reference.
fn input(front_only: bool, cull: bool, phase: f32, revision: u64) -> UiRenderInput {
    let geometry = box_uv_geometry(front_only);
    let vertices: Vec<_> = geometry
        .vertices
        .iter()
        .map(|vertex| UiRenderVertex {
            position: [vertex.position[0], vertex.position[1]],
            clip_z: vertex.position[2],
            clip_w: 1.,
            uv: vertex.uv,
            color: [255; 4],
            style_flags: render_api::UI_STYLE_MODEL_VERTEX,
            alpha_cutoff: -1.,
            model_light: 1.,
            overlay_color: [0.; 4],
        })
        .collect();
    let count = vertices.len() as u32;
    let mut bone = UiModelBone {
        screen: (glam::Mat4::from_cols_array_2d(&mask_screen())
            * glam::Mat4::from_scale(glam::Vec3::new(-1., 1., 1.)))
        .to_cols_array_2d(),
        normal: [[0.; 4]; 3],
        overlay: [0.; 4],
        uv_rect: [
            0.,
            0.,
            BOX_TEXTURE_SIZE[0] as f32,
            BOX_TEXTURE_SIZE[1] as f32,
        ],
        uv_transform: [phase, 0., 1., 1.],
        uv_cross: [0.; 4],
        tint: [1.; 4],
        styles: [0, 1, 0, 0],
    };
    bone.set_backface_culling(cull);
    UiRenderInput {
        revision,
        viewport_size: [SIDE; 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: (0..count).collect(),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, SIDE, SIDE),
            0,
            count,
            render_model::UI_BLEND_ALPHA,
        )
        .with_depth_test(true)
        .with_depth_write(true)
        .with_isolated_depth_scope(Some(1))]),
        models: Arc::from([UiModelPose {
            scope: 1,
            bones: Arc::from([bone]),
            bounds: UiScissor::new(0, 0, SIDE, SIDE),
        }]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned(BOX_TEXTURE_SIZE, box_uv_pixels()).unwrap()],
                1,
            )
            .unwrap(),
        ),
    }
}

#[test]
fn culled_box_uv_planes_match_front_artwork_at_every_texture_phase() {
    let Some(render) = renderer() else { return };
    let (mut app, _, image) = app_with_effects(render, |app| {
        app.add_plugins(super::UiRenderPlugin);
    });
    let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    let pipelines = FixturePipelines {
        checker: fixture_pipeline(device, 1, "checker"),
        sloped_checker: fixture_pipeline(device, 1, "sloped_checker"),
        silhouette: fixture_pipeline(device, 1, "silhouette"),
        scene: 0,
    };
    app.sub_app_mut(RenderApp).insert_resource(pipelines);
    let stats = app
        .world()
        .resource::<render_scene::UiRenderStatsResource>()
        .clone();
    let mut revision = 0;
    let mut two_sided_differences = 0;
    for phase in [0., 0.25, 0.5, 0.75] {
        let mut images = Vec::new();
        for (front_only, cull) in [(true, false), (false, true), (false, false)] {
            revision += 1;
            app.world_mut()
                .resource_mut::<render_scene::UiRenderSceneResource>()
                .publish(input(front_only, cull, phase, revision), &stats)
                .unwrap();
            for _ in 0..8 {
                frame(&mut app);
            }
            images.push(read_pixels(&app, &image));
        }
        let mismatches: Vec<_> = images[0]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(images[1].as_chunks::<4>().0.iter())
            .enumerate()
            .filter(|(_, (front, box_pixel))| front != box_pixel)
            .take(4)
            .map(|(index, (front, box_pixel))| {
                (
                    index % SIDE as usize,
                    index / SIDE as usize,
                    front.to_vec(),
                    box_pixel.to_vec(),
                )
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "culled box differs from its front face at phase {phase}: {mismatches:?}"
        );
        two_sided_differences += usize::from(images[0] != images[2]);
    }
    assert!(
        two_sided_differences >= 2,
        "two-sided materials retain their opposite-face artwork"
    );
}
