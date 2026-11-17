use super::*;
use bevy::{
    asset::uuid_handle,
    render::{
        render_resource::{
            ColorTargetState, ColorWrites, FragmentState, RenderPipelineDescriptor, TextureFormat,
            VertexState,
        },
        renderer::RenderDevice,
    },
};
use render_scene::PipelineWarmupReadiness;

const UNLOADED_SHADER: Handle<Shader> = uuid_handle!("5b0e7c3d-8a41-4f2e-9d6b-1c7a2e9f4b80");

/// Validation-only backend with synchronous compilation, so `process_queue` finishes each pipeline.
fn cache() -> PipelineCache {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&default())).unwrap();
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&default())).unwrap();

    let mut cache = PipelineCache::new(RenderDevice::from(device), true);
    cache.set_shader(
        Handle::<Shader>::default().id(),
        crate::shader_safety::from_wgsl(
            "@vertex fn vertex() -> @builtin(position) vec4f { return vec4f(0.0); }\n\
             @fragment fn fragment() -> @location(0) vec4f { return vec4f(1.0); }",
            "warmup-test.wgsl",
        ),
    );
    cache
}

fn descriptor(shader: Handle<Shader>, write_mask: ColorWrites) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask,
            })],
            ..default()
        }),
        ..default()
    }
}

#[derive(Resource)]
struct TwoModes {
    shader: Handle<Shader>,
    calls: usize,
}

impl PrewarmPipelines for TwoModes {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        self.calls += 1;
        for mask in [ColorWrites::RED, ColorWrites::GREEN] {
            let mut descriptor = descriptor(self.shader.clone(), mask);
            descriptor.multisample.count = view.msaa.samples();
            ids.push(cache.queue_render_pipeline(descriptor));
        }
        Ok(())
    }
}

const VIEW: WarmView = WarmView {
    msaa: Msaa::Off,
    hdr: false,
    enhanced: false,
    shader_pack: false,
    output: None,
};

fn world(shader: Handle<Shader>) -> World {
    let mut world = World::new();
    world.insert_resource(cache());
    world.insert_resource(TwoModes { shader, calls: 0 });
    world.insert_resource(WarmupRegistry {
        views: vec![VIEW],
        ..default()
    });
    world
}

fn ready(world: &mut World) -> bool {
    world.resource_scope(|world, cache: Mut<PipelineCache>| {
        registered_pipelines_ready(&cache, &mut world.resource_mut::<WarmupRegistry>())
    })
}

/// Runs one owner's frame with the current-view readiness reset performed by collection.
fn warm_frame(world: &mut World, owner: bevy::ecs::system::SystemId) {
    world.resource_mut::<WarmupRegistry>().ids.clear();
    world.run_system(owner).unwrap();
}

#[test]
fn loading_waits_for_every_queued_variant_and_steady_frames_do_not_rewarm() {
    let mut world = world(Handle::default());
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    warm_frame(&mut world, warm);
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 2);
    assert!(!ready(&mut world), "queued pipelines are not usable yet");
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));
    for _ in 0..3 {
        warm_frame(&mut world, warm);
    }
    assert_eq!(world.resource::<TwoModes>().calls, 1);
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 2);
}

#[test]
fn a_new_view_configuration_warms_again_and_closes_the_gate() {
    let mut world = world(Handle::default());
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    warm_frame(&mut world, warm);
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));
    world.resource_mut::<WarmupRegistry>().views.push(WarmView {
        msaa: Msaa::Sample4,
        ..VIEW
    });
    warm_frame(&mut world, warm);
    assert_eq!(world.resource::<TwoModes>().calls, 2);
    assert!(!ready(&mut world));
}

#[test]
fn loading_prepares_single_sample_coverage_before_msaa_is_disabled() {
    let mut world = world(Handle::default());
    world.resource_mut::<WarmupRegistry>().views[0].msaa = Msaa::Sample4;
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    warm_frame(&mut world, warm);
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 4);
    assert!(!ready(&mut world));
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));

    world.resource_mut::<WarmupRegistry>().views[0].msaa = Msaa::Off;
    warm_frame(&mut world, warm);
    assert_eq!(world.resource::<WarmupRegistry>().ids.len(), 2);
    assert!(
        ready(&mut world),
        "Performance coverage was already compiled"
    );
}

#[test]
fn unloaded_shaders_hold_loading() {
    let mut world = world(UNLOADED_SHADER);
    let warm = world.register_system(prewarm_owner::<TwoModes>);
    warm_frame(&mut world, warm);
    for _ in 0..2 {
        world.resource_mut::<PipelineCache>().process_queue();
        assert!(!ready(&mut world));
    }
}

/// Cancelling a request must release readiness for the already compiled active view.
#[test]
fn cancelled_sample_requests_do_not_hold_ready_views() {
    let mut app = App::new();
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    app.insert_sub_app(RenderApp, render);
    register::<TwoModes>(&mut app);
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(cache());
    world.insert_resource(TwoModes {
        shader: Handle::default(),
        calls: 0,
    });
    world.spawn((
        crate::render_test_support::camera(false),
        Msaa::Off,
        Camera3d::default(),
    ));
    world.run_schedule(Render);
    world.resource_mut::<PipelineCache>().process_queue();
    world.run_schedule(Render);
    assert!(world.resource::<PipelineWarmupReadiness>().is_ready());

    world.resource_mut::<TwoModes>().shader = UNLOADED_SHADER;
    world
        .resource_mut::<SampleCountPreflight>()
        .request(Some(SampleCountRequest {
            samples: Msaa::Sample8,
            hdr: false,
            enhanced: false,
        }));
    world.run_schedule(Render);
    assert!(!world.resource::<PipelineWarmupReadiness>().is_ready());

    world.resource_mut::<SampleCountPreflight>().request(None);
    world.run_schedule(Render);
    assert!(world.resource::<PipelineWarmupReadiness>().is_ready());
}

#[test]
fn no_view_never_releases_loading() {
    let cache = cache();
    assert!(!registered_pipelines_ready(
        &cache,
        &mut WarmupRegistry::default()
    ));
    assert!(!PipelineWarmupReadiness::default().is_ready());
}

/// Upscaling's blocking blit wait in `Prepare` starts every queued pipeline, so warm-up must
/// queue after it and still before `Render` starts this frame's queue.
#[test]
fn warmup_queues_after_the_blocking_blit_and_before_render() {
    #[derive(Resource, Default)]
    struct Queued(Vec<usize>);
    fn probe(registry: Res<WarmupRegistry>, mut queued: ResMut<Queued>) {
        queued.0.push(registry.ids.len());
    }
    let mut app = App::new();
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    app.insert_sub_app(RenderApp, render);
    register::<TwoModes>(&mut app);
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(cache())
        .insert_resource(TwoModes {
            shader: Handle::default(),
            calls: 0,
        })
        .init_resource::<Queued>()
        .add_systems(
            Render,
            (
                probe.in_set(RenderSystems::Prepare),
                probe.in_set(RenderSystems::Render),
            ),
        );
    render
        .world_mut()
        .spawn((crate::render_test_support::camera(false), Msaa::Off));
    render.world_mut().run_schedule(Render);
    assert_eq!(render.world().resource::<Queued>().0, [0, 2]);
}

/// An owner the menu draws with, registered ahead of world owners.
#[derive(Resource, Default)]
struct MenuOwner {
    ids: WarmupIds,
}

impl PrewarmPipelines for MenuOwner {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        _view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        let id = cache.queue_render_pipeline(descriptor(Handle::default(), ColorWrites::BLUE));
        self.ids.push(id);
        ids.push(id);
        Ok(())
    }
}

/// A menu launch's render app with one interface owner, one world owner and one world camera.
fn staged_app(admission: WorldWorkAdmission) -> App {
    let mut app = App::new();
    app.insert_resource(admission);
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    app.insert_sub_app(RenderApp, render);
    register_interface::<MenuOwner>(&mut app);
    register::<TwoModes>(&mut app);
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(cache());
    world.init_resource::<MenuOwner>();
    world.insert_resource(TwoModes {
        shader: Handle::default(),
        calls: 0,
    });
    world.spawn((
        crate::render_test_support::camera(false),
        Msaa::Off,
        Camera3d::default(),
    ));
    app
}

/// Runs one render frame, then compiles what it queued as `RenderSystems::Render` would.
fn staged_frame(app: &mut App) {
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(Render);
    world.resource_mut::<PipelineCache>().process_queue();
}

fn world_owner_calls(app: &App) -> usize {
    app.sub_app(RenderApp).world().resource::<TwoModes>().calls
}

fn menu_owner_variants(app: &App) -> WarmupIds {
    let owner = app.sub_app(RenderApp).world().resource::<MenuOwner>();
    owner.ids.clone()
}

#[test]
fn interface_variants_settle_before_any_world_variant_is_queued() {
    use render_scene::startup_work::WorldGpuStart;
    let admission = WorldWorkAdmission::after_menu(WorldGpuStart::AfterMenu);
    let mut app = staged_app(admission.clone());
    let readiness = app.world().resource::<PipelineWarmupReadiness>().clone();
    staged_frame(&mut app);
    assert!(
        menu_owner_variants(&app).is_empty(),
        "nothing before the menu"
    );
    assert_eq!(world_owner_calls(&app), 0);

    admission.menu_presented();
    app.sub_app_mut(RenderApp).world_mut().run_schedule(Render);
    let menu = menu_owner_variants(&app);
    assert_eq!(menu.len(), 1, "the interface owner queues first");
    assert_eq!(
        world_owner_calls(&app),
        0,
        "world variants wait while an interface variant is still compiling"
    );
    assert!(!admission.is_admitted());
    assert!(!readiness.is_ready());

    let world = app.sub_app_mut(RenderApp).world_mut();
    world.resource_mut::<PipelineCache>().process_queue();
    world.run_schedule(Render);
    assert!(admission.is_admitted());
    assert_eq!(world_owner_calls(&app), 1);
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    assert!(
        menu.iter()
            .all(|&id| progress::settled(cache.get_render_pipeline_state(id))),
        "every interface variant had settled when the world owner queued"
    );
    assert!(!readiness.is_ready(), "the world variants are only queued");
    staged_frame(&mut app);
    staged_frame(&mut app);
    assert!(readiness.is_ready());
}

#[test]
fn a_join_policy_compiles_nothing_behind_the_menu_and_warms_on_a_join() {
    use render_scene::startup_work::WorldGpuStart;
    let admission = WorldWorkAdmission::after_menu(WorldGpuStart::OnJoin);
    let mut app = staged_app(admission.clone());
    let readiness = app.world().resource::<PipelineWarmupReadiness>().clone();
    admission.menu_presented();
    for _ in 0..4 {
        staged_frame(&mut app);
    }
    assert!(menu_owner_variants(&app).is_empty());
    assert_eq!(world_owner_calls(&app), 0);
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    assert_eq!(cache.pipelines().count(), 0, "the idle menu queued nothing");
    assert!(!readiness.is_ready());

    admission.join_requested();
    staged_frame(&mut app);
    assert_eq!(menu_owner_variants(&app).len(), 1);
    assert_eq!(world_owner_calls(&app), 0, "the interface still goes first");
    for _ in 0..3 {
        staged_frame(&mut app);
    }
    assert_eq!(world_owner_calls(&app), 1);
    assert!(readiness.is_ready(), "a join's loading gate still releases");
}

/// Nothing is warmed for either count while shader work waits for a join, so holding the old
/// count would only make the join compile both.
#[test]
fn a_sample_count_change_before_the_first_join_switches_without_warming() {
    use render_scene::startup_work::WorldGpuStart;
    let request = SampleCountRequest {
        samples: Msaa::Sample4,
        hdr: false,
        enhanced: false,
    };
    let admission = WorldWorkAdmission::after_menu(WorldGpuStart::OnJoin);
    let mut app = staged_app(admission.clone());
    admission.menu_presented();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world
        .resource_mut::<SampleCountPreflight>()
        .request(Some(request));
    world.run_schedule(Render);
    assert!(world.resource::<SampleCountPreflight>().is_ready(request));
    assert_eq!(world_owner_calls(&app), 0);

    // Once a join has started warm-up, the same request waits for its variants again.
    admission.join_requested();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.run_schedule(Render);
    assert!(!world.resource::<SampleCountPreflight>().is_ready(request));
    for _ in 0..4 {
        staged_frame(&mut app);
    }
    let world = app.sub_app(RenderApp).world();
    assert!(world.resource::<SampleCountPreflight>().is_ready(request));
}
