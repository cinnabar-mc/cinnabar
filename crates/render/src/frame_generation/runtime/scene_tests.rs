//! Production scene, SMAA, name and native UI nodes keep names out of interpolation inputs.
use super::*;
use crate::depth_smaa::tests::raster_tests::{app_with_effects, ready as smaa_ready};
use crate::frame_generation::optical_flow::tests::read;
use crate::render_test_support::scene_raster::{
    FixturePipelines, fixture_pipeline, frame, read_pixels, renderer,
};
use bevy::render::{
    render_resource::Extent3d, sync_world::RenderEntity, view::ViewDepthStencilTexture,
};
use render_model::NametagRecord;

/// The smallest output the interpolation backend admits.
const SIZE: [u32; 2] = [256, 64];
/// Fixture world depth written by the checker pipeline.
const WORLD_DEPTH: f32 = 0.5;

#[derive(Resource, Default)]
struct LateReadiness(bool);

/// Publishes the marker after preparation, as a worker finishing after phase queueing would.
/// Image targets are never admitted, so this also keeps their native UI target in use.
fn publish_late_readiness(
    mut commands: Commands,
    ready: Res<LateReadiness>,
    views: Query<Entity, With<ViewState>>,
    natives: Query<&crate::ui_render::target::NativeUiTarget>,
) {
    for native in &natives {
        native.resume();
    }
    for entity in &views {
        if ready.0 {
            commands.entity(entity).insert(FrameGenerationActive);
        } else {
            commands.entity(entity).remove::<FrameGenerationActive>();
        }
    }
}

/// A depth-tested name in front of the checker, so its glyphs write depth when drawn.
fn name() -> NametagRecord {
    NametagRecord {
        anchor: [0.0, 0.0, -0.15],
        rect: [-12.0, -8.0, 12.0, 8.0],
        uv: [0.0, 0.0, -1.0, -1.0],
        color: [0.5, 0.25, 0.75, 1.0],
        depth_tested: 1,
        text: 1,
        ..default()
    }
}

#[test]
fn names_stay_out_of_interpolated_colour_and_depth_across_late_toggles() {
    for smaa in [false, true] {
        for msaa in [Msaa::Off, Msaa::Sample4] {
            if !scene_case(smaa, msaa) {
                return;
            }
        }
    }
}

/// Returns false when no native adapter is available.
fn scene_case(smaa: bool, msaa: Msaa) -> bool {
    let Some(render) = renderer() else {
        return false;
    };
    let (mut app, camera, image) = app_with_effects(render, |app| {
        crate::lighting::install(app);
        crate::nametag_render::install_nametag_render(app);
        crate::chunk::transparent::gamma_pass::register_fixture_commands(app);
        app.add_plugins((crate::ui_render::UiRenderPlugin, FrameGenerationPlugin));
        app.sub_app_mut(RenderApp)
            .init_resource::<LateReadiness>()
            .add_systems(
                Render,
                publish_late_readiness
                    .in_set(RenderSystems::PrepareResources)
                    .after(prepare::views),
            );
    });
    {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let device = world.resource::<RenderDevice>().clone();
        world.insert_resource(FixturePipelines {
            checker: fixture_pipeline(&device, msaa.samples(), "checker"),
            sloped_checker: fixture_pipeline(&device, msaa.samples(), "sloped_checker"),
            silhouette: fixture_pipeline(&device, msaa.samples(), "silhouette"),
            scene: 0,
        });
        crate::chunk::transparent::gamma_pass::install_graph(world);
    }
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&image)
        .unwrap()
        .resize(Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        });
    app.world_mut().entity_mut(camera).insert((
        msaa,
        FrameGenerationCamera {
            reset_epoch: 0,
            interval_ns: Some(16_000_000),
            capture: false,
            near: 0.1,
            far: f32::INFINITY,
            fov_y: 1.0,
        },
    ));
    if smaa {
        app.world_mut()
            .entity_mut(camera)
            .insert(bevy::anti_alias::smaa::Smaa::default());
        smaa_ready(&mut app);
    }
    let mut render_entity = None;
    test_time::eventually("native UI target prepared", || {
        frame(&mut app);
        let entity = app.world().get::<RenderEntity>(camera).unwrap().id();
        render_entity = Some(entity);
        app.sub_app(RenderApp)
            .world()
            .get::<crate::ui_render::target::NativeUiTarget>(entity)
            .is_some_and(|native| native.is_ready())
    });
    let entity = render_entity.unwrap();
    for _ in 0..8 {
        frame(&mut app);
    }
    let background = read_pixels(&app, &image);
    app.world_mut()
        .resource_mut::<render_scene::NametagSceneResource>()
        .records = vec![name()];
    for _ in 0..8 {
        frame(&mut app);
    }
    assert_ne!(
        read_pixels(&app, &image),
        background,
        "the name must be visible in ordinary output (SMAA {smaa}, {msaa:?})"
    );
    let ready = {
        let world = app.sub_app(RenderApp).world();
        let device = world.resource::<RenderDevice>().clone();
        let depth = world.get::<ViewDepthStencilTexture>(entity).unwrap();
        let source = crate::scene_sampling::view_depth(depth).clone();
        let output = world
            .get::<ViewTarget>(entity)
            .unwrap()
            .out_texture_view_format()
            .unwrap();
        let key = gpu::Key {
            device: device.wgpu_device().clone(),
            size: FrameGenerationSize {
                display: SIZE,
                render: SIZE,
            },
            samples: msaa.samples(),
            output,
            depth: source.id(),
        };
        Arc::new(gpu::Ready::new(device, key, source).expect("prepared frame generation scene"))
    };
    let render = app.sub_app_mut(RenderApp).world_mut();
    render
        .entity_mut(entity)
        .insert(ViewState(Arc::clone(&ready)));
    render.resource_mut::<LateReadiness>().0 = true;
    for _ in 0..3 {
        frame(&mut app);
    }
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>().wgpu_device();
    let queue = world.resource::<RenderQueue>();
    let clean = read(device, queue, &ready.world, 4);
    // World history holds encoded bytes; the fixture's primaries encode identically.
    assert!(
        clean
            .as_chunks::<4>()
            .0
            .iter()
            .zip(background.as_chunks::<4>().0)
            .all(|(a, b)| a[..3].iter().zip(&b[..3]).all(|(a, b)| a.abs_diff(*b) <= 2)),
        "names must never enter interpolated colour (SMAA {smaa}, {msaa:?})"
    );
    let depth = read(device, queue, &ready.depth._texture, 4);
    assert!(
        depth
            .as_chunks::<4>()
            .0
            .iter()
            .all(|bytes| (f32::from_le_bytes(*bytes) - WORLD_DEPTH).abs() < 0.00001),
        "glyph depth must not replace interpolated world depth (SMAA {smaa}, {msaa:?})"
    );
    let real = read(device, queue, &ready._real, 4);
    assert_ne!(real, clean, "current names must still reach the real image");
    let render = app.sub_app_mut(RenderApp).world_mut();
    render.resource_mut::<LateReadiness>().0 = false;
    frame(&mut app);
    app.sub_app_mut(RenderApp)
        .world_mut()
        .resource_mut::<LateReadiness>()
        .0 = true;
    frame(&mut app);
    true
}

/// Builds the production scene with UI and frame generation, sized for the backend.
fn plain_scene(render: bevy::render::RenderPlugin) -> (App, Entity, Handle<Image>) {
    let (mut app, camera, image) =
        crate::render_test_support::scene_raster::app_with_effects(render, |app| {
            app.add_plugins((crate::ui_render::UiRenderPlugin, FrameGenerationPlugin));
        });
    let world = app.sub_app_mut(RenderApp).world_mut();
    let device = world.resource::<RenderDevice>().clone();
    world.insert_resource(FixturePipelines {
        checker: fixture_pipeline(&device, 1, "checker"),
        sloped_checker: fixture_pipeline(&device, 1, "sloped_checker"),
        silhouette: fixture_pipeline(&device, 1, "silhouette"),
        scene: 0,
    });
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&image)
        .unwrap()
        .resize(Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        });
    (app, camera, image)
}

/// A paced 60 FPS request, as the client publishes it with the setting on.
fn request() -> FrameGenerationCamera {
    FrameGenerationCamera {
        reset_epoch: 0,
        interval_ns: Some(16_666_666),
        capture: false,
        near: 0.1,
        far: f32::INFINITY,
        fov_y: 1.0,
    }
}

#[test]
fn setting_off_frames_present_once_and_toggles_release_interpolation_state() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = plain_scene(render);
    frame(&mut app);
    let entity = app.world().get::<RenderEntity>(camera).unwrap().id();
    let polls = |app: &App| {
        app.sub_app(RenderApp)
            .world()
            .resource::<crate::device_poll::DevicePolls>()
            .0
    };
    let first = polls(&app);
    for frames in 1..=4 {
        frame(&mut app);
        let world = app.sub_app(RenderApp).world();
        assert_eq!(polls(&app), first + frames, "one device poll per frame");
        assert!(
            world
                .resource::<crate::device_poll::FrameWait>()
                .0
                .is_none()
        );
        assert!(world.resource::<Replay>().0.is_none());
        let view = world.entity(entity);
        assert!(
            !view.contains::<FrameGenerationCamera>()
                && !view.contains::<FrameGenerationActive>()
                && !view.contains::<ViewState>()
                && !view.contains::<crate::ui_render::target::NativeUiTarget>(),
            "the setting off allocates nothing and holds no surface"
        );
    }

    app.world_mut().entity_mut(camera).insert(request());
    test_time::eventually("native UI target prepared", || {
        frame(&mut app);
        app.sub_app(RenderApp)
            .world()
            .get::<crate::ui_render::target::NativeUiTarget>(entity)
            .is_some()
    });
    let ready = {
        let world = app.sub_app(RenderApp).world();
        let device = world.resource::<RenderDevice>().clone();
        let depth = world.get::<ViewDepthStencilTexture>(entity).unwrap();
        let source = crate::scene_sampling::view_depth(depth).clone();
        let output = world
            .get::<ViewTarget>(entity)
            .unwrap()
            .out_texture_view_format()
            .unwrap();
        let key = gpu::Key {
            device: device.wgpu_device().clone(),
            size: FrameGenerationSize {
                display: SIZE,
                render: SIZE,
            },
            samples: 1,
            output,
            depth: source.id(),
        };
        Arc::new(gpu::Ready::new(device, key, source).expect("prepared frame generation"))
    };
    app.sub_app_mut(RenderApp)
        .world_mut()
        .entity_mut(entity)
        .insert(ViewState(Arc::clone(&ready)));

    // An image target is never admitted, so its native target only serves interpolation.
    frame(&mut app);
    let world = app.sub_app(RenderApp).world();
    assert!(!world.entity(entity).contains::<FrameGenerationActive>());
    assert!(
        !crate::ui_render::target::UiTarget::new(
            world.get::<ViewTarget>(entity).unwrap(),
            world.get::<crate::ui_render::target::NativeUiTarget>(entity),
        )
        .is_native(),
        "a bypassed frame draws screen UI directly"
    );

    // A live screenshot reaches the render camera as a capture, which admission rejects.
    app.world_mut()
        .spawn(bevy::render::view::screenshot::Screenshot::image(image));
    frame(&mut app);
    let captured = *app
        .sub_app(RenderApp)
        .world()
        .get::<FrameGenerationCamera>(entity)
        .unwrap();
    assert!(captured.capture);
    let mut presenter = present::Presenter::default();
    assert!(
        !presenter.admit(&captured, true),
        "screenshots present real frames"
    );
    assert!(presenter.admit(&request(), true));

    app.world_mut()
        .entity_mut(camera)
        .remove::<FrameGenerationCamera>();
    frame(&mut app);
    frame(&mut app);
    let view = app.sub_app(RenderApp).world().entity(entity);
    assert!(
        !view.contains::<FrameGenerationCamera>()
            && !view.contains::<FrameGenerationActive>()
            && !view.contains::<ViewState>()
            && !view.contains::<crate::ui_render::target::NativeUiTarget>(),
        "turning the setting off releases every interpolation resource"
    );
    assert_eq!(
        Arc::strong_count(&ready),
        1,
        "no prepared state outlives the toggle"
    );
}
