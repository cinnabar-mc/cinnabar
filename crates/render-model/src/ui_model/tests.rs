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
