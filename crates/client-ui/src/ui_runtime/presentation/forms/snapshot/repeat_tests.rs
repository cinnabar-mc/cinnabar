//! Repeat coordinates and page ownership must survive presentation snapshots.

use super::*;
use render_api::ui_repeat::{UI_STYLE_REPEAT, UI_STYLE_REPEAT_PAGE, UI_STYLE_REPEAT_X};
use render_model::{UiRenderBatch, UiScissor, UiTextureCatalog, UiTexturePage};
use std::sync::Arc;

/// Builds a repeated region with one ordinary quad for each physical source page.
fn input(pages: Vec<UiTexturePage>, repeats: f32, flags: u16) -> UiRenderInput {
    let page_count = pages.len();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut batches = Vec::new();
    for page in 0..page_count {
        let base = vertices.len() as u32;
        vertices.extend(
            [[0., 0.], [6., 0.], [6., 2.], [0., 2.]].map(|position| UiRenderVertex {
                position,
                clip_z: 0.,
                clip_w: 1.,
                uv: [position[0] * repeats / 6., position[1] / 2.],
                color: [255; 4],
                style_flags: u32::from(flags),
                alpha_cutoff: -1.,
                model_light: 1.,
                overlay_color: [0., 0., 2., 2.],
            }),
        );
        let first = indices.len() as u32;
        indices.extend([0, 1, 2, 0, 2, 3].map(|index| base + index));
        batches.push(UiRenderBatch::new(
            page as u32,
            UiScissor::new(0, 0, 6, 2),
            first,
            6,
            render_model::UI_BLEND_ALPHA,
        ));
    }
    UiRenderInput {
        models: Default::default(),
        revision: 1,
        viewport_size: [6, 2],
        safe_area: [0; 4],
        vertices: vertices.into(),
        indices: indices.into(),
        batches: batches.into(),
        textures: Arc::new(UiTextureCatalog::new(pages, page_count).unwrap()),
    }
}

#[test]
fn repeated_snapshot_regions_preserve_clipping_phase_and_atlas_relocation() {
    let red = [160, 20, 30, 255];
    let blue = [30, 60, 240, 255];
    let row = [[0, 255, 0, 255], red, blue, [255, 255, 0, 255]];
    let page = UiTexturePage::owned([4, 2], row.repeat(2).concat().into()).unwrap();
    let mut input = input(vec![page], 3., UI_STYLE_REPEAT_X);
    for vertex in Arc::make_mut(&mut input.vertices) {
        vertex.uv[0] += 0.375;
    }
    Arc::make_mut(&mut input.batches)[0].scissor = UiScissor::new(1, 0, 4, 2);
    let resident: Vec<_> = input
        .vertices
        .iter()
        .map(|&source| render_model::FontAtlasVertex {
            source,
            atlas_offset: [1., 0.],
        })
        .collect();
    let image = rasterize_resident(&input, &resident);
    for (x, _, pixel) in image.enumerate_pixels() {
        let expected = match x {
            1 | 3 => red,
            2 | 4 => blue,
            _ => [70, 90, 110, 255],
        };
        assert_eq!(pixel.0, expected, "repeated source at column {x}");
    }
}

#[test]
fn repeated_snapshot_page_halos_filter_without_duplicate_translucent_coverage() {
    let row = [[160, 0, 0, 128], [0, 0, 240, 128]];
    let page = UiTexturePage::owned([2, 2], row.repeat(2).concat().into()).unwrap();
    let pages = (0..2)
        .map(|column| {
            page.clone()
                .with_repeat_content([column, 0, column + 1, 2])
                .unwrap()
        })
        .collect();
    let image = rasterize(&input(
        pages,
        1.5,
        UI_STYLE_REPEAT | UI_STYLE_REPEAT_PAGE | u16::from(render_api::UI_STYLE_BILINEAR),
    ));
    for (x, _, pixel) in image.enumerate_pixels() {
        let uv = ((x as f32 + 0.5) * 0.5).rem_euclid(2.0);
        let mix = (uv - 0.5).clamp(0., 1.);
        for (channel, background) in [70., 90., 110.].into_iter().enumerate() {
            let source = f32::from(row[0][channel]) * (1. - mix) + f32::from(row[1][channel]) * mix;
            let expected = (source * 128. / 255. + background * 127. / 255.).round() as i16;
            assert!((i16::from(pixel[channel]) - expected).abs() <= 1);
        }
    }
}

#[test]
fn repeated_controller_overlay_keeps_source_phase_and_alpha_in_snapshots() {
    let red = [160, 20, 30, 255];
    let blue = [30, 60, 240, 255];
    let page = UiTexturePage::owned([2, 2], [red, blue].repeat(2).concat().into()).unwrap();
    let mut input = input(vec![page], 3., UI_STYLE_REPEAT);
    let bone = render_model::UiModelBone {
        screen: glam::Mat4::IDENTITY.to_cols_array_2d(),
        normal: [[0.; 4]; 3],
        overlay: [0.; 4],
        uv_rect: [0., 0., 2., 2.],
        uv_transform: [0., 0., 1., 1.],
        uv_cross: [0.; 4],
        tint: [1.; 4],
        styles: [0, 1, 0, 0],
    };
    for vertex in Arc::make_mut(&mut input.vertices) {
        vertex.style_flags |= render_api::UI_STYLE_MODEL_VERTEX;
        vertex.overlay_color = [0.; 4];
    }
    Arc::make_mut(&mut input.batches)[0].isolated_depth_scope = Some(1);
    input.models = Arc::from([render_model::UiModelPose {
        scope: 1,
        bones: Arc::from([bone]),
        bounds: UiScissor::new(0, 0, 6, 2),
    }]);
    let original = rasterize(&input);
    Arc::make_mut(&mut Arc::make_mut(&mut input.models)[0].bones)[0].overlay = [1., 0., 0., 0.5];
    let tinted = rasterize(&input);
    for x in 0..6 {
        let source = if x % 2 == 0 { red } else { blue };
        assert_eq!(original.get_pixel(x, 0).0, source, "source repeat at {x}");
        let expected = [
            (f32::from(source[0]) * 0.5 + 127.5).round() as u8,
            (f32::from(source[1]) * 0.5).round() as u8,
            (f32::from(source[2]) * 0.5).round() as u8,
            source[3],
        ];
        assert_eq!(
            tinted.get_pixel(x, 0).0,
            expected,
            "independent overlay at {x}"
        );
    }
}

#[test]
fn culled_box_uv_snapshots_match_front_artwork_and_keep_two_sided_materials() {
    use render_model::actor::fixture::{
        BOX_TEXTURE_SIZE, box_uv_geometry, box_uv_pixels, mask_screen,
    };
    let image = |front_only, cull, phase| {
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
        let mut bone = render_model::UiModelBone {
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
        rasterize(&UiRenderInput {
            revision: 1,
            viewport_size: [64; 2],
            safe_area: [0; 4],
            vertices: vertices.into(),
            indices: (0..count).collect(),
            batches: Arc::from([UiRenderBatch::new(
                0,
                UiScissor::new(0, 0, 64, 64),
                0,
                count,
                render_model::UI_BLEND_ALPHA,
            )
            .with_isolated_depth_scope(Some(1))]),
            models: Arc::from([render_model::UiModelPose {
                scope: 1,
                bones: Arc::from([bone]),
                bounds: UiScissor::new(0, 0, 64, 64),
            }]),
            textures: Arc::new(
                UiTextureCatalog::new(
                    vec![UiTexturePage::owned(BOX_TEXTURE_SIZE, box_uv_pixels()).unwrap()],
                    1,
                )
                .unwrap(),
            ),
        })
    };
    let mut two_sided_differences = 0;
    for phase in [0., 0.25, 0.5, 0.75] {
        let front = image(true, false, phase);
        assert_eq!(
            image(false, true, phase),
            front,
            "culled snapshot phase {phase}"
        );
        two_sided_differences += usize::from(image(false, false, phase) != front);
    }
    assert!(two_sided_differences >= 2);
}
