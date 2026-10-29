//! Counts actual production system executions through their ECS change ticks.

use super::*;
use crate::shaderpack::plugin;
use bevy::{ecs::schedule::ScheduleLabel, render::ExtractSchedule};
use std::collections::HashMap;

/// Installs the production main, extraction and render schedules without a GPU or carriers.
fn fixture() -> App {
    let mut app = App::new();
    app.init_resource::<Assets<Shader>>();
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    render.init_schedule(ExtractSchedule);
    app.insert_sub_app(bevy::render::RenderApp, render);
    plugin::build(&mut app);
    plugin::finish(&mut app);
    app
}

/// Runs one schedule and counts systems whose last-executed tick advanced.
fn executions(world: &mut World, label: impl ScheduleLabel) -> usize {
    world.schedule_scope(label, |world, schedule| {
        schedule.initialize(world).unwrap();
        let before = schedule
            .systems()
            .unwrap()
            .map(|(id, system)| (id, system.get_last_run()))
            .collect::<HashMap<_, _>>();
        schedule.run(world);
        schedule
            .systems()
            .unwrap()
            .filter(|(id, system)| {
                system.system_type() != std::any::TypeId::of::<bevy::ecs::schedule::ApplyDeferred>()
                    && before[id] != system.get_last_run()
            })
            .count()
    })
}

/// Runs extraction with the same main-world handoff the renderer uses.
fn extract(app: &mut App) -> usize {
    let mut main = MainWorld::default();
    std::mem::swap(app.world_mut(), &mut *main);
    let render = app.sub_app_mut(bevy::render::RenderApp).world_mut();
    render.insert_resource(main);
    let count = executions(render, ExtractSchedule);
    let mut main = render.remove_resource::<MainWorld>().unwrap();
    std::mem::swap(app.world_mut(), &mut *main);
    count
}

#[test]
fn idle_frame_executes_no_shader_pack_systems() {
    let mut app = fixture();
    assert_eq!(executions(app.world_mut(), Update), 0);
    assert_eq!(executions(app.world_mut(), PostUpdate), 0);
    assert_eq!(extract(&mut app), 0);
    assert_eq!(
        executions(app.sub_app_mut(bevy::render::RenderApp).world_mut(), Render),
        0
    );
}

#[test]
fn off_retires_a_load_then_returns_to_zero_systems() {
    let mut app = fixture();
    let path = std::path::PathBuf::from("fixture");
    app.world_mut().resource_mut::<ShaderPackRequest>().pack = Some(path.clone());
    assert!(executions(app.world_mut(), Update) > 0);
    let (send, receive) = std::sync::mpsc::channel();
    let worker = app
        .world_mut()
        .resource_mut::<render_scene::shaderpack::ShaderPackLoadProgress>()
        .start();
    let loader = &mut *app.world_mut().resource_mut::<Loader>();
    loader.key = Some((path, None, 0, 0));
    loader.job = Some(crate::shaderpack::load_driver::Job::new(receive, worker));
    app.world_mut().resource_mut::<ShaderPackRequest>().pack = None;
    assert!(executions(app.world_mut(), Update) > 0);
    assert!(app.world().resource::<Loader>().job.is_some());
    send.send(Err("cancelled".into())).unwrap();
    assert!(executions(app.world_mut(), Update) > 0);
    assert!(app.world().resource::<Loader>().job.is_none());
    app.world_mut().clear_trackers();
    assert_eq!(executions(app.world_mut(), Update), 0);
    assert_eq!(extract(&mut app), 0);
    assert_eq!(
        executions(app.sub_app_mut(bevy::render::RenderApp).world_mut(), Render),
        0
    );
}

#[test]
fn loaded_pack_releases_render_owners_then_returns_to_zero_systems() {
    use crate::shaderpack::{
        geometry::ShaderpackGeometry, kernels::Kernels, plugin::ShaderPackViews,
    };
    use bevy::render::renderer::{PendingCommandBuffers, RenderAdapter, RenderDevice, RenderQueue};
    let mut app = fixture();
    let mut ready = plugin::native_tests::ready_world();
    let render = app.sub_app_mut(bevy::render::RenderApp).world_mut();
    render.insert_resource(ready.remove_resource::<RenderDevice>().unwrap());
    render.insert_resource(ready.remove_resource::<RenderQueue>().unwrap());
    render.insert_resource(ready.remove_resource::<RenderAdapter>().unwrap());
    render.insert_resource(ready.remove_resource::<LoadedShaderPack>().unwrap());
    render.init_resource::<PendingCommandBuffers>();
    let camera = render.spawn(ShaderPackRendering).id();
    assert!(executions(render, Render) > 0);
    assert!(render.contains_resource::<Kernels>());
    assert!(render.contains_resource::<ShaderpackGeometry>());
    render.clear_trackers();
    render.resource_mut::<LoadedShaderPack>().0 = None;
    render.despawn(camera);
    assert!(executions(render, Render) > 0);
    assert!(!render.contains_resource::<Kernels>());
    assert!(!render.contains_resource::<ShaderpackGeometry>());
    assert!(
        render
            .resource::<ShaderPackViews>()
            .states
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(!render.resource::<ShaderpackGeometrySettings>().enabled);
    render.clear_trackers();
    assert_eq!(executions(render, Render), 0);
}

#[test]
fn standalone_geometry_extracts_shared_shaders_until_off_releases_its_cache() {
    use crate::shaderpack::geometry::ShaderpackGeometryPlugin;
    use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderQueue};
    let mut app = App::new();
    app.init_resource::<Assets<Shader>>()
        .add_message::<AssetEvent<Shader>>();
    let mut render = SubApp::new();
    render.add_schedule(Render::base_schedule());
    render.init_schedule(ExtractSchedule);
    app.insert_sub_app(bevy::render::RenderApp, render);
    app.add_plugins(ShaderpackGeometryPlugin);
    let mut ready = plugin::native_tests::ready_world();
    let render = app.sub_app_mut(bevy::render::RenderApp).world_mut();
    render.insert_resource(ready.remove_resource::<RenderDevice>().unwrap());
    render.insert_resource(ready.remove_resource::<RenderQueue>().unwrap());
    render.insert_resource(ready.remove_resource::<RenderAdapter>().unwrap());
    render.resource_mut::<ShaderpackGeometrySettings>().enabled = true;
    assert!(executions(render, Render) > 0);
    assert!(render.contains_resource::<Kernels>());
    assert!(extract(&mut app) > 0);
    let render = app.sub_app_mut(bevy::render::RenderApp).world_mut();
    render.resource_mut::<ShaderpackGeometrySettings>().enabled = false;
    assert!(executions(render, Render) > 0);
    assert!(!render.contains_resource::<Kernels>());
    render.clear_trackers();
    assert_eq!(extract(&mut app), 0);
    assert_eq!(
        executions(app.sub_app_mut(bevy::render::RenderApp).world_mut(), Render),
        0
    );
}
