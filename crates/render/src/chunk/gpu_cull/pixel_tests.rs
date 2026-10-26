//! Completed terrain frames exercise uploads, constrained tables and draw submission together.

use super::app_tests::{frame, render_plugin_with_storage_limit};
use super::*;
use crate::render_test_support::scene_raster::read_pixels;
use bevy::{asset::Handle, camera::RenderTarget, core_pipeline::tonemapping::Tonemapping};
use std::sync::Arc;

/// A native renderer with exactly the storage and indirect features under test.
fn fixture(features: WgpuFeatures, storage: u32) -> Option<(App, Entity, Handle<Image>)> {
    eprintln!("terrain fixture device: storage {storage}, features {features:?}");
    let render =
        render_plugin_with_storage_limit(wgpu::Backends::PRIMARY, features, Some(storage))?;
    eprintln!("terrain fixture installing app");
    let (mut app, camera) =
        app_tests::chunk_app_with(render, Msaa::Off, camera_views()[0], |app| {
            app.insert_resource(render_scene::ChunkTextureAssets::with_revision(
                Arc::clone(crate::chunk::tests::opaque_runtime_assets()),
                0,
            ));
            app.world_mut()
                .resource_mut::<render_scene::ChunkOcclusionCulling>()
                .0 = false;
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            for (_, image) in images.iter_mut() {
                image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            }
        });
    app.world_mut().entity_mut(camera).insert(Tonemapping::None);
    let RenderTarget::Image(image) = app.world().get::<RenderTarget>(camera).unwrap() else {
        panic!("terrain fixture requires an image target");
    };
    let image = image.handle.clone();
    eprintln!("terrain fixture warming pipelines");
    test_time::eventually("terrain fixture pipelines", || {
        frame(&mut app);
        app.world()
            .resource::<render_scene::PipelineWarmupReadiness>()
            .is_ready()
    });
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapter>()
        .get_info();
    eprintln!(
        "terrain pixel fixture: {:?}, {}, storage {storage}, features {features:?}",
        info.backend, info.name
    );
    Some((app, camera, image))
}

/// Queues solid sections with the same artwork installed in the rendering fixture.
fn insert_terrain(app: &mut App) {
    for key in app_tests::KEYS {
        app.world_mut()
            .resource_mut::<render_scene::ChunkRenderQueue>()
            .try_insert(
                key,
                crate::chunk::tests::solid_test_mesh(),
                render_scene::ChunkUploadPriority::new(0.0),
            )
            .unwrap();
    }
}

/// Opposing views expose different faces and avoid depending on one camera direction.
fn camera_views() -> [Transform; 2] {
    [Vec3::new(-24.0, 32.0, -24.0), Vec3::new(56.0, 32.0, 40.0)]
        .map(|eye| Transform::from_translation(eye).looking_at(Vec3::new(16.0, 8.0, 8.0), Vec3::Y))
}

/// Waits for the queued sections to have drawable GPU storage and bindings.
fn uploaded(app: &mut App, count: usize) {
    test_time::eventually("drawable terrain uploads", || {
        frame(app);
        let arena = app.sub_app(RenderApp).world().resource::<ChunkGpuArena>();
        arena.allocations.len() == count && arena.bind_group.is_some()
    });
    frame(app);
}

/// Identifies terrain by its difference from the same camera's empty completed frame.
fn coverage(pixels: &[u8], empty: &[u8]) -> Vec<bool> {
    assert_eq!(pixels.len(), empty.len());
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(empty.as_chunks::<4>().0)
        .map(|(pixel, background)| pixel != background)
        .collect()
}

/// Reports coverage loss without dumping an entire frame's pixel mask on failure.
fn assert_coverage(expected: &[bool], actual: &[bool], context: impl std::fmt::Display) {
    assert_eq!(expected.len(), actual.len());
    let different = expected.iter().zip(actual).filter(|(a, b)| a != b).count();
    assert_eq!(different, 0, "terrain coverage differs: {context}");
}

/// Waits for a stable opaque control, excluding an output target's uninitialized contents.
fn empty_frame(app: &mut App, image: &Handle<Image>) -> Vec<u8> {
    let mut previous = Vec::new();
    test_time::eventually("completed empty terrain frame", || {
        frame(app);
        let pixels = read_pixels(app, image);
        let ready = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
            && pixels == previous;
        previous = pixels;
        ready
    });
    previous
}

/// Writes optional completed frames and adapter details for physical-device acceptance.
fn capture(app: &App, image: &Handle<Image>, name: &str, pixels: &[u8]) {
    let Some(root) = std::env::var_os("CINNABAR_TERRAIN_CAPTURE_DIR") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir_all(&root).unwrap();
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>();
    let adapter = world.resource::<RenderAdapter>();
    let info = adapter.get_info();
    let indirect = device
        .features()
        .contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE);
    let storage = device.limits().max_storage_buffers_per_shader_stage;
    let name = format!("{name}-storage{storage}-indirect{indirect}");
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(image)
        .unwrap()
        .texture_descriptor
        .size;
    image::save_buffer_with_format(
        root.join(format!("{name}.png")),
        pixels,
        size.width,
        size.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
    let metadata = serde_json::json!({
        "adapter": info.name,
        "backend": format!("{:?}", info.backend),
        "device_type": format!("{:?}", info.device_type),
        "driver": info.driver,
        "driver_info": info.driver_info,
        "storage_buffers": storage,
        "indirect_feature": indirect,
        "draw_mode": format!("{:?}", select_chunk_draw_mode(
            adapter.get_downlevel_capabilities().flags,
            device.features(),
            Backends::from(info.backend),
            direct_draws_required(&info),
        )),
        "gpu_culling": world.resource::<GpuCullSupport>().0,
        "direct_occlusion": world.resource::<DirectOcclusionSupport>().0,
        "width": size.width,
        "height": size.height,
    });
    std::fs::write(
        root.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
}

/// Captures uploaded terrain from both views, with an empty-frame control for each.
fn terrain_frames(app: &mut App, camera: Entity, image: &Handle<Image>) -> Vec<Vec<bool>> {
    let views = camera_views();
    let empty = views.map(|view| {
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = view;
        empty_frame(app, image)
    });
    insert_terrain(app);
    uploaded(app, app_tests::KEYS.len());
    views
        .into_iter()
        .zip(empty)
        .enumerate()
        .map(|(index, (view, empty))| {
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = view;
            frame(app);
            let pixels = read_pixels(app, image);
            capture(app, image, &format!("view{index}"), &pixels);
            let mask = coverage(&pixels, &empty);
            assert!(
                mask.iter().any(|covered| *covered),
                "terrain frame is empty"
            );
            assert!(
                mask.iter().any(|covered| !*covered),
                "fixture lost its background control"
            );
            mask
        })
        .collect()
}

/// Direct and indirect draws retain the same visible terrain with either table layout.
#[test]
fn terrain_pixels_match_across_storage_limits_and_draw_modes() {
    let full = required_vertex_storage_buffers();
    let compact = minimum_vertex_storage_buffers();
    let Some((mut reference, camera, image)) =
        fixture(WgpuFeatures::empty(), full).or_else(|| fixture(WgpuFeatures::empty(), compact))
    else {
        eprintln!("missing fixture: native GPU for terrain pixel comparisons");
        return;
    };
    let expected = terrain_frames(&mut reference, camera, &image);
    drop(reference);
    for (features, storage) in [
        (WgpuFeatures::empty(), compact),
        (WgpuFeatures::INDIRECT_FIRST_INSTANCE, full),
        (WgpuFeatures::INDIRECT_FIRST_INSTANCE, compact),
    ] {
        let Some((mut app, camera, image)) = fixture(features, storage) else {
            eprintln!(
                "missing fixture: native GPU supporting {features:?} and {storage} storage bindings"
            );
            continue;
        };
        let actual = terrain_frames(&mut app, camera, &image);
        for (view, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
            assert_coverage(
                expected,
                actual,
                format_args!("view {view}, storage {storage}, features {features:?}"),
            );
        }
    }
}

/// Occlusion toggles and repeated uploads cannot leave a previously visible scene blank.
#[test]
fn terrain_pixels_survive_occlusion_toggles_and_reuploads() {
    for (features, storage) in [
        (WgpuFeatures::empty(), minimum_vertex_storage_buffers()),
        (
            WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            required_vertex_storage_buffers(),
        ),
    ] {
        let Some((mut app, _, image)) = fixture(features, storage) else {
            eprintln!(
                "missing fixture: native GPU supporting {features:?} and {storage} storage bindings"
            );
            continue;
        };
        assert_reuploads(&mut app, &image);
    }
}

/// Preserves coverage through occlusion changes and complete residency replacement.
fn assert_reuploads(app: &mut App, image: &Handle<Image>) {
    let empty = empty_frame(app, image);
    insert_terrain(app);
    uploaded(app, app_tests::KEYS.len());
    let pixels = read_pixels(app, image);
    capture(app, image, "resident", &pixels);
    let expected = coverage(&pixels, &empty);
    assert!(
        expected.iter().any(|covered| *covered),
        "terrain frame is empty"
    );
    assert!(
        expected.iter().any(|covered| !*covered),
        "fixture lost its background control"
    );
    for enabled in [true, false, true] {
        app.world_mut()
            .resource_mut::<render_scene::ChunkOcclusionCulling>()
            .0 = enabled;
        frame(app);
        let pixels = read_pixels(app, image);
        capture(app, image, &format!("occlusion-{enabled}"), &pixels);
        assert_coverage(
            &expected,
            &coverage(&pixels, &empty),
            format_args!("occlusion enabled: {enabled}"),
        );
    }
    for key in app_tests::KEYS {
        app.world_mut()
            .resource_mut::<render_scene::ChunkRenderQueue>()
            .try_remove(key)
            .unwrap();
    }
    test_time::eventually("removed terrain pixels", || {
        frame(app);
        read_pixels(app, image) == empty
    });
    insert_terrain(app);
    uploaded(app, app_tests::KEYS.len());
    let pixels = read_pixels(app, image);
    capture(app, image, "reuploaded", &pixels);
    assert_coverage(&expected, &coverage(&pixels, &empty), "terrain reuploaded");
}
