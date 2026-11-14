use super::*;
use bevy::{ecs::schedule::SingleThreadedExecutor, image::ToExtents};

/// Uses the production preparation systems with one ordinary single-sample camera.
fn ordinary_view() -> (World, Entity, Schedule) {
    let (mut world, view, mut schedule) = msaa_view_fixture();
    world.entity_mut(view).insert(Msaa::Off);
    schedule.set_executor(SingleThreadedExecutor::new());
    (world, view, schedule)
}

/// Checks actual image ownership rather than only the requested sample count.
fn assert_shared(world: &World, view: Entity, shared: bool) {
    let scene = world.get::<SceneTarget>(view).unwrap();
    let target = world.get::<ViewTarget>(view).unwrap();
    assert_eq!(scene.texture.id() == target.main_texture().id(), shared);
    assert_eq!(scene.shares_view_target, shared);
    assert!(!unavailable(world, view));
}

#[test]
fn ordinary_single_sample_needs_no_scene_texture_or_finish_commands() {
    let (mut world, view, mut schedule) = ordinary_view();
    let allocations = std::rc::Rc::new(std::cell::Cell::new(0));
    let _observe = crate::gpu_allocation::inject({
        let allocations = allocations.clone();
        move |label, _| {
            if label == "shared main-pass colour" {
                allocations.set(allocations.get() + 1);
            }
            false
        }
    });
    schedule.run(&mut world);
    assert_shared(&world, view, true);
    assert_eq!(allocations.get(), 0);
    let scene = world.get::<SceneTarget>(view).unwrap();
    let original = (scene.texture.id(), scene.encoded.id(), scene.linear.id());
    let device = world.resource::<RenderDevice>().clone();
    let ((), commands) =
        crate::render_test_support::record(&mut world, &device, |world, context| {
            let scene = world.get::<SceneTarget>(view).unwrap();
            let target = world.get::<ViewTarget>(view).unwrap();
            scene.finish(context, world, target);
            scene.finish(context, world, target);
            assert!(scene.resolved.load(Ordering::Relaxed));
        });
    assert!(
        commands.is_empty(),
        "sharing must encode neither a copy nor a resolve"
    );
    schedule.run(&mut world);
    let scene = world.get::<SceneTarget>(view).unwrap();
    assert_eq!(
        (scene.texture.id(), scene.encoded.id(), scene.linear.id()),
        original
    );
    assert!(
        !world
            .get::<SceneTarget>(view)
            .unwrap()
            .resolved
            .load(Ordering::Relaxed)
    );
    assert_eq!(allocations.get(), 0);
}

/// An extracted effect must force separate storage before its pipelines become ready.
fn check_effect<T: Component>(world: &mut World, view: Entity, schedule: &mut Schedule, effect: T) {
    world.entity_mut(view).insert(effect);
    schedule.run(world);
    assert_shared(world, view, false);
    world.entity_mut(view).remove::<T>();
    schedule.run(world);
    assert_shared(world, view, true);
}

#[test]
fn extracted_effects_hdr_msaa_and_retained_cameras_keep_owned_colour() {
    let (mut world, view, mut schedule) = ordinary_view();
    check_effect(
        &mut world,
        view,
        &mut schedule,
        render_scene::CameraMotionBlur {
            exposure_seconds: 0.0,
            samples: 1,
            reset_epoch: 0,
            delta_seconds: 0.0,
        },
    );
    check_effect(
        &mut world,
        view,
        &mut schedule,
        render_scene::EnhancedRendering::default(),
    );
    check_effect(
        &mut world,
        view,
        &mut schedule,
        render_scene::shaderpack::ShaderPackRendering,
    );
    check_effect(&mut world, view, &mut schedule, Smaa::default());
    world.get_mut::<ExtractedCamera>(view).unwrap().hdr = true;
    schedule.run(&mut world);
    assert_shared(&world, view, false);
    world.get_mut::<ExtractedCamera>(view).unwrap().hdr = false;
    world.entity_mut(view).insert(Msaa::Sample4);
    schedule.run(&mut world);
    assert_shared(&world, view, false);
    assert_eq!(
        world
            .get::<SceneTarget>(view)
            .unwrap()
            .texture
            .sample_count(),
        4
    );
    world.entity_mut(view).insert(Msaa::Off);
    world.get_mut::<ExtractedCamera>(view).unwrap().clear_color = ClearColorConfig::None;
    schedule.run(&mut world);
    assert_shared(&world, view, false);
    world.get_mut::<ExtractedCamera>(view).unwrap().clear_color = ClearColorConfig::Default;
    schedule.run(&mut world);
    assert_shared(&world, view, true);
    world
        .get_mut::<CameraMainTextureUsages>(view)
        .unwrap()
        .0
        .remove(TextureUsages::RENDER_ATTACHMENT);
    schedule.run(&mut world);
    assert_shared(&world, view, false);
}

#[test]
fn shared_colour_tracks_ping_pong_resize_and_same_size_replacement() {
    let (mut world, view, mut schedule) = ordinary_view();
    schedule.run(&mut world);
    let first = world.get::<SceneTarget>(view).unwrap().texture.id();
    for _ in 0..3 {
        world.get::<ViewTarget>(view).unwrap().post_process_write();
        let current = world.get::<ViewTarget>(view).unwrap().main_texture().id();
        schedule.run(&mut world);
        assert_shared(&world, view, true);
        assert_eq!(
            world.get::<SceneTarget>(view).unwrap().texture.id(),
            current
        );
    }
    let previous = world.get::<SceneTarget>(view).unwrap().texture.id();
    assert_ne!(first, previous);
    world.get_mut::<CameraMainTextureUsages>(view).unwrap().0 |= TextureUsages::COPY_DST;
    schedule.run(&mut world);
    assert_shared(&world, view, true);
    assert_ne!(
        world.get::<SceneTarget>(view).unwrap().texture.id(),
        previous
    );
    for dimensions in [UVec2::new(7, 3), UVec2::ONE] {
        world
            .get_mut::<ExtractedCamera>(view)
            .unwrap()
            .physical_target_size = Some(dimensions);
        schedule.run(&mut world);
        assert_shared(&world, view, true);
        assert_eq!(
            world.get::<SceneTarget>(view).unwrap().texture.size(),
            dimensions.to_extents()
        );
    }
}

#[test]
fn cameras_with_shared_postprocess_images_keep_independent_scene_storage() {
    let (mut world, first, mut schedule) = ordinary_view();
    let target = world
        .get::<ExtractedCamera>(first)
        .unwrap()
        .target
        .clone()
        .unwrap();
    let second = spawn_view(&mut world, target, render_scene::SCENE_COLOR_FORMAT);
    world.entity_mut(second).insert(Msaa::Off);
    schedule.run(&mut world);
    assert_shared(&world, first, false);
    assert_shared(&world, second, false);
    assert_ne!(
        world.get::<SceneTarget>(first).unwrap().texture.id(),
        world.get::<SceneTarget>(second).unwrap().texture.id()
    );
    // Different usages give the second camera independent Bevy images at the same dimensions.
    world.get_mut::<CameraMainTextureUsages>(second).unwrap().0 |= TextureUsages::COPY_DST;
    schedule.run(&mut world);
    assert_shared(&world, first, true);
    assert_shared(&world, second, true);
    assert_ne!(
        world.get::<SceneTarget>(first).unwrap().texture.id(),
        world.get::<SceneTarget>(second).unwrap().texture.id()
    );
    world.entity_mut(second).remove::<ViewTarget>();
    world
        .get_mut::<ExtractedCamera>(second)
        .unwrap()
        .physical_target_size = None;
    schedule.run(&mut world);
    assert!(world.get::<SceneTarget>(second).is_none());
    assert_shared(&world, first, true);
}

#[test]
fn shared_view_allocation_failure_recovers_and_effect_toggles_retry_immediately() {
    let (mut world, view, mut schedule) = ordinary_view();
    let failure = crate::gpu_allocation::inject(|label, _| label == "shared scene colour view");
    schedule.run(&mut world);
    assert!(unavailable(&world, view));
    assert!(world.get::<SceneTarget>(view).is_none());
    drop(failure);
    for _ in 0..8 {
        schedule.run(&mut world);
    }
    assert_shared(&world, view, true);
    world.entity_mut(view).insert(Smaa::default());
    let failure = crate::gpu_allocation::inject(|label, _| label == "shared main-pass colour");
    schedule.run(&mut world);
    assert!(unavailable(&world, view));
    drop(failure);
    world.entity_mut(view).remove::<Smaa>();
    schedule.run(&mut world);
    assert_shared(&world, view, true);
}

#[test]
fn shared_view_backoff_survives_postprocess_flips_and_retries_replaced_images() {
    let (mut world, view, mut schedule) = ordinary_view();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
    let failure = crate::gpu_allocation::inject({
        let attempts = attempts.clone();
        move |label, _| {
            let shared = label == "shared scene colour view";
            attempts.set(attempts.get() + usize::from(shared));
            shared
        }
    });
    let mut original_pair = None;
    for _ in 0..6 {
        schedule.run(&mut world);
        assert!(unavailable(&world, view));
        let target = world.get::<ViewTarget>(view).unwrap();
        let mut pair = [target.main_texture().id(), target.main_texture_other().id()];
        pair.sort_unstable();
        assert_eq!(*original_pair.get_or_insert(pair), pair);
        target.post_process_write();
    }
    assert_eq!(
        attempts.get(),
        2,
        "the same pair must back off despite flips"
    );
    world.get_mut::<CameraMainTextureUsages>(view).unwrap().0 |= TextureUsages::COPY_DST;
    schedule.run(&mut world);
    assert_eq!(attempts.get(), 3, "a replacement must retry immediately");
    drop(failure);
    for _ in 0..8 {
        world.get::<ViewTarget>(view).unwrap().post_process_write();
        schedule.run(&mut world);
    }
    assert_shared(&world, view, true);
}

/// Draws partial opaque, encoded translucent and hand coverage through production attachments.
fn rendered_pixels(
    device: &RenderDevice,
    queue: &wgpu::Queue,
    shared: bool,
    encoded_clear: bool,
) -> [[u8; 4]; 4] {
    let (mut world, view, mut schedule) = view_fixture(device.clone());
    world.entity_mut(view).insert(Msaa::Off);
    world.get_mut::<CameraMainTextureUsages>(view).unwrap().0 |= TextureUsages::COPY_DST;
    world.get_mut::<ExtractedCamera>(view).unwrap().clear_color =
        ClearColorConfig::Custom(Color::linear_rgba(0.017, 0.13, 0.41, 0.62));
    if !shared {
        world
            .entity_mut(view)
            .insert(render_scene::EnhancedRendering::default());
    }
    schedule.run(&mut world);
    assert_shared(&world, view, shared);
    let format = world.get::<SceneTarget>(view).unwrap().format();
    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shared scene byte equivalence"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    return vec4f(vec2f(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2f(1.0), 0.0, 1.0);
}
@fragment fn opaque(@builtin(position) p: vec4f) -> @location(0) vec4f {
    if p.x > 1.0 || p.y > 1.0 { discard; }
    return vec4f(0.07, 0.29, 0.58, 0.83);
}
@fragment fn transparent(@builtin(position) p: vec4f) -> @location(0) vec4f {
    if p.y > 1.0 { discard; }
    return vec4f(0.23, 0.61, 0.11, 0.37);
}
@fragment fn hand(@builtin(position) p: vec4f) -> @location(0) vec4f {
    if p.x < 1.0 || p.y < 1.0 { discard; }
    return vec4f(0.53, 0.19, 0.03, 1.0);
}
"#
                .into(),
            ),
        });
    let opaque = pipeline(device, &shader, format, 1, "opaque", None);
    let transparent = pipeline(
        device,
        &shader,
        format.remove_srgb_suffix(),
        1,
        "transparent",
        Some(wgpu::BlendState::ALPHA_BLENDING),
    );
    let hand = pipeline(device, &shader, format, 1, "hand", None);
    let ((), commands) =
        crate::render_test_support::record(&mut world, device, |world, context| {
            let scene = world.get::<SceneTarget>(view).unwrap();
            let target = world.get::<ViewTarget>(view).unwrap();
            // Exercise Bevy's first-clear flag through either colour space, even with no geometry.
            {
                let attachments = [Some(scene.color_attachment(target, encoded_clear))];
                assert!(matches!(
                    attachments[0].as_ref().unwrap().ops.load,
                    LoadOp::Clear(_)
                ));
                context
                    .command_encoder()
                    .begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &attachments,
                        ..default()
                    });
            }
            for (encoded, pipeline) in [(false, &opaque), (true, &transparent), (false, &hand)] {
                let attachments = [Some(scene.color_attachment(target, encoded))];
                assert!(matches!(
                    attachments[0].as_ref().unwrap().ops.load,
                    LoadOp::Load
                ));
                let mut pass = context
                    .command_encoder()
                    .begin_render_pass(&RenderPassDescriptor {
                        color_attachments: &attachments,
                        ..default()
                    });
                pass.set_pipeline(pipeline);
                pass.draw(0..3, 0..1);
            }
            scene.finish(context, world, target);
        });
    queue.submit(commands);
    let target = world.get::<ViewTarget>(view).unwrap();
    read_pixels(
        device,
        queue,
        device.create_command_encoder(&default()),
        target.main_texture(),
    )
}

#[test]
fn shared_and_copied_scene_bytes_match_for_clears_blending_and_partial_coverage() {
    let Some((device, queue, _)) = fixture() else {
        return;
    };
    for encoded_clear in [false, true] {
        let owned = rendered_pixels(&device, &queue, false, encoded_clear);
        let shared = rendered_pixels(&device, &queue, true, encoded_clear);
        assert_eq!(shared, owned, "first clear encoded={encoded_clear}");
        assert!(
            owned.windows(2).all(|pixels| pixels[0] != pixels[1]),
            "fixture must contain different coverage and colour: {owned:?}"
        );
        assert_eq!(owned[3][3], 255);
        assert!(owned[2][3] < 255);
    }
}
