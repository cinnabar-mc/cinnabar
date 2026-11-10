//! Model palette validation and immutable publication reuse.
use super::*;
use crate::{
    UiRenderBatch, UiRenderInput, UiRenderRejectReason, UiRenderScene, UiRenderStats,
    UiTextureCatalog, UiTexturePage,
};

/// Builds a finite palette-addressed triangle with original white artwork.
fn input() -> UiRenderInput {
    let bone = UiModelBone {
        screen: Mat4::IDENTITY.to_cols_array_2d(),
        normal: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        overlay: [0.; 4],
        uv_rect: [0., 0., 1., 1.],
        uv_transform: [0., 0., 1., 1.],
        uv_cross: [0.; 4],
        tint: [1.; 4],
        styles: [0; 4],
    };
    let vertex = UiRenderVertex {
        position: [0.; 2],
        clip_z: 0.,
        clip_w: 1.,
        uv: [0.; 2],
        color: [255; 4],
        style_flags: render_api::UI_STYLE_MODEL_VERTEX,
        alpha_cutoff: 0.5,
        model_light: 1.,
        overlay_color: [0., 0., 1., 0.],
    };
    UiRenderInput {
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: Arc::from([vertex; 3]),
        indices: Arc::from([0, 1, 2]),
        batches: Arc::from(
            [UiRenderBatch::new(0, UiScissor::new(0, 0, 64, 64), 0, 3, 0)
                .with_depth_test(true)
                .with_depth_write(true)
                .with_isolated_depth_scope(Some(7))],
        ),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
        models: Arc::from([UiModelPose {
            scope: 7,
            bones: Arc::from([bone]),
            bounds: UiScissor::new(0, 0, 64, 64),
        }]),
    }
}

/// Invalid palette changes cannot bypass admission through retained geometry.
#[test]
fn retained_model_updates_reject_missing_and_nonfinite_bones() {
    let previous = input();
    previous.validate().unwrap();
    for malformed in 0..4 {
        let mut next = previous.clone();
        match malformed {
            0 => next.models = Arc::from([]),
            1 => Arc::make_mut(&mut next.models)[0].scope = 9,
            2 => {
                Arc::make_mut(&mut Arc::make_mut(&mut next.models)[0].bones)[0].screen[0][0] =
                    f32::NAN
            }
            _ => {
                Arc::make_mut(&mut Arc::make_mut(&mut next.models)[0].bones)[0].tint[0] =
                    f32::INFINITY
            }
        }
        assert_eq!(
            next.validate_update(&previous),
            Err(UiRenderRejectReason::InvalidModelPose)
        );
    }
    let mut styled = previous;
    Arc::make_mut(&mut Arc::make_mut(&mut styled.models)[0].bones)[0].styles[0] = u32::MAX;
    assert!(
        styled.models[0].bones[0].is_finite(),
        "integer style bits are not floating payload"
    );
}

/// Render snapshots and weak witnesses keep retired headers immutable until all readers release them.
#[test]
fn model_publications_recycle_only_unshared_headers() {
    let mut scene = UiRenderScene::default();
    let stats = UiRenderStats::default();
    let mut frame = input();
    for revision in 1..5 {
        let reader = scene.input.clone();
        frame.revision = revision;
        scene.publish(frame.clone(), &stats).unwrap();
        drop(reader);
    }
    let held = scene.input.clone().unwrap();
    let weak = Arc::downgrade(&held);
    let previous = held.clone();
    frame.revision += 1;
    let (_, allocations) = test_alloc::count(|| scene.publish(frame, &stats).unwrap());
    assert_eq!(allocations, 0);
    assert_eq!(held.revision, previous.revision);
    assert!(Arc::ptr_eq(&weak.upgrade().unwrap(), &held));
    assert_ne!(scene.revision, held.revision);
}

/// Retired headers must not extend the lifetime of a replaced texture catalog.
#[test]
fn header_pool_releases_replaced_texture_catalogs() {
    let mut pool = crate::ui_publication_pool::UiPublicationPool::default();
    let first = pool.publish(None, input());
    let retired = Arc::downgrade(&first.textures);
    let mut next = first.as_ref().clone();
    next.textures = Arc::new(next.textures.as_ref().clone());
    let current = pool.publish(Some(Arc::clone(&first)), next);
    drop(first);
    assert!(retired.upgrade().is_none());
    current.validate().unwrap();
}

/// Opt-in crate benchmark separates full vertex admission from a retained palette update.
#[test]
fn ui_model_pose_publication_cost() {
    if std::env::var_os("CINNABAR_HUD_MOTION_BENCH").is_none() {
        eprintln!("missing-fixture: opt-in UI palette benchmark; set CINNABAR_HUD_MOTION_BENCH=1");
        return;
    }
    let mut previous = input();
    let count = 4096 * 3;
    previous.vertices = vec![previous.vertices[0]; count].into();
    previous.indices = (0..count as u32).collect::<Vec<_>>().into();
    Arc::make_mut(&mut previous.batches)[0].index_count = count as u32;
    previous.validate().unwrap();
    let next = previous.clone();
    const SAMPLES: usize = 1000;
    let started = std::time::Instant::now();
    for _ in 0..SAMPLES {
        std::hint::black_box(next.validate()).unwrap();
    }
    let full = started.elapsed().as_secs_f64() * 1000. / SAMPLES as f64;
    let started = std::time::Instant::now();
    for _ in 0..SAMPLES {
        std::hint::black_box(next.validate_update(&previous)).unwrap();
    }
    let palette = started.elapsed().as_secs_f64() * 1000. / SAMPLES as f64;
    eprintln!(
        "UI_MODEL_POSE full_validation_ms={full:.6} palette_validation_ms={palette:.6} vertices={count} bones={}",
        next.models[0].bones.len()
    );
}

#[test]
fn repeated_model_uvs_keep_tile_coordinates_until_fragment_sampling() {
    let mut bone = input().models[0].bones[0];
    bone.uv_rect = [12., 8., 44., 16.];
    bone.uv_transform = [0.25, 0., 2., 1.];
    bone.styles[1] = 1;
    bone.overlay = [0.8, 0.1, 0.2, 0.5];
    bone.set_backface_culling(true);
    let mut vertex = input().vertices[0];
    vertex.uv = [0., 0.5];
    let first = bone.project(vertex);
    vertex.uv = [1., 0.5];
    let last = bone.project(vertex);
    assert_eq!(first.uv, [0.25, 0.5]);
    assert_eq!(last.uv, [2.25, 0.5]);
    assert_eq!(first.overlay_color, bone.overlay);
    let pose = UiModelPose {
        scope: 7,
        bones: Arc::from([bone]),
        bounds: UiScissor::new(0, 0, 64, 64),
    };
    let projected = project_ui_model_vertex(vertex, Some(&pose)).unwrap();
    assert_eq!(projected.vertex.overlay_color, bone.overlay);
    assert_eq!(projected.repeat_bounds, Some([12., 8., 32., 8.]));
    assert!(projected.cull_back_faces);
    let middle = [(first.uv[0] + last.uv[0]) / 2., 0.5];
    assert_eq!(
        render_api::ui_repeat::source_uv(
            middle,
            projected.repeat_bounds.unwrap(),
            first.style_flags as u16
        ),
        [20., 12.]
    );
}

#[test]
fn ordinary_repeated_regions_retain_their_source_metadata() {
    let mut vertex = input().vertices[0];
    vertex.style_flags = u32::from(render_api::ui_repeat::UI_STYLE_REPEAT);
    vertex.overlay_color = [12., 8., 32., 8.];
    let projected = project_ui_model_vertex(vertex, None).unwrap();
    assert_eq!(projected.repeat_bounds, Some(vertex.overlay_color));
    assert_eq!(projected.vertex, vertex);
    assert!(!projected.cull_back_faces);
}

#[test]
fn selected_model_texture_pages_grow_and_release_within_catalog_budgets() {
    let blank = UiTexturePage::owned([256; 2], vec![0; 256 * 256 * 4].into()).unwrap();
    let fixed = super::super::UI_SELECTED_MODEL_PAGE_OFFSET;
    let mut pages = vec![blank.clone(); fixed + 1];
    let art = UiTexturePage::owned(
        [crate::UI_ART_PAGE_SIDE; 2],
        vec![0; (crate::UI_ART_PAGE_SIDE * crate::UI_ART_PAGE_SIDE * 4) as usize].into(),
    )
    .unwrap();
    for page in &mut pages[1 + crate::MAX_UI_DYNAMIC_PAGES..1 + crate::UI_NATIVE_SHEET_PAGE_OFFSET]
    {
        *page = art.clone();
    }
    pages[1 + crate::UI_LOCAL_FONT_PAGE_OFFSET] = UiTexturePage::owned(
        [crate::UI_LOCAL_FONT_PAGE_SIDE; 2],
        vec![0; (crate::UI_LOCAL_FONT_PAGE_SIDE * crate::UI_LOCAL_FONT_PAGE_SIDE * 4) as usize]
            .into(),
    )
    .unwrap();
    let catalog = crate::UiTextureCatalog::new(pages.clone(), 1).unwrap();
    let selected = UiTexturePage::owned([32, 8], vec![210; 32 * 8 * 4].into()).unwrap();
    let mut dynamic = pages[1..].to_vec();
    dynamic.push(selected.clone());
    let extended = catalog.replace_dynamic(dynamic).unwrap();
    assert_eq!(extended.pages().last(), Some(&selected));
    assert_ne!(extended.identity(), catalog.identity());
    assert_eq!(extended.static_identity(), catalog.static_identity());
    let released = extended
        .replace_dynamic(extended.pages()[1..fixed + 1].to_vec())
        .unwrap();
    assert_eq!(released.pages().len(), catalog.pages().len());
    assert_eq!(released.static_identity(), catalog.static_identity());
    assert!(
        released.replace_dynamic(Vec::new()).is_err(),
        "selected ownership cannot remove the fixed producers"
    );
}
