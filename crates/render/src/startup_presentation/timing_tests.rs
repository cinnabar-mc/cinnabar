//! The startup recorder samples pipelines after Bevy's final queue pass.
use crate::startup_presentation::timing::{compile, install};
use bevy::render::render_resource::{CachedComputePipelineId, ComputePipelineDescriptor};
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache},
    },
};
use render_scene::startup_timing::StartupTiming;
use web_time::Instant;

/// A compute pipeline queued after the startup queue pass, so only Bevy's final pass builds it.
#[derive(Resource)]
struct Late {
    shader: Handle<bevy::shader::Shader>,
    frames: u32,
    id: Option<CachedComputePipelineId>,
}

/// Queues once the shader has had frames to reach the render world.
fn queue_late(mut late: ResMut<Late>, cache: Res<PipelineCache>) {
    late.frames += 1;
    if late.frames == 3 {
        late.id = Some(cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("late".into()),
            shader: late.shader.clone(),
            ..default()
        }));
    }
}

/// A pipeline the final queue pass builds is sampled ready in the frame that drew with it.
#[test]
fn pipelines_built_by_the_final_queue_pass_are_ready_in_that_frame() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::window::WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((
            bevy::asset::AssetPlugin::default(),
            TransformPlugin,
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
        ))
        .add_plugins(bevy::render::RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..crate::chunk::noop_render_plugin(bevy::render::render_resource::WgpuFeatures::empty())
        });
    install(&mut app);
    let shader = app
        .world_mut()
        .resource_mut::<Assets<bevy::shader::Shader>>()
        .add(crate::shader_safety::from_wgsl(
            "@compute @workgroup_size(1) fn main() {}",
            file!(),
        ));
    let timing = StartupTiming::detached(Instant::now());
    app.sub_app_mut(RenderApp)
        .insert_resource(timing.clone())
        .insert_resource(Late {
            shader,
            frames: 0,
            id: None,
        })
        .add_systems(
            Render,
            queue_late.after(compile).before(RenderSystems::Render),
        );
    app.finish();
    app.cleanup();
    test_time::eventually("late pipeline queued", || {
        app.update();
        app.sub_app(RenderApp)
            .world()
            .resource::<Late>()
            .id
            .is_some()
    });
    let world = app.sub_app(RenderApp).world();
    let id = world.resource::<Late>().id.unwrap();
    assert!(matches!(
        world
            .resource::<PipelineCache>()
            .get_compute_pipeline_state(id),
        CachedPipelineState::Ok(_)
    ));
    let report: serde_json::Value =
        serde_json::from_str(&timing.finish(Instant::now()).unwrap()).unwrap();
    let late = report["pipelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pipeline| pipeline["label"] == "late")
        .expect("the late pipeline was sampled in its frame");
    assert!(late["ready_ms"].is_f64(), "{late}");
}
