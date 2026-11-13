//! Driver failures injected into the noop backend, through Bevy's cache and the direct helper.

use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::{Handle, Shader, World, default},
    render::{
        render_resource::{
            CachedRenderPipelineId, FragmentState, RenderPipelineDescriptor, TextureFormat,
            VertexState,
        },
        renderer::RenderDevice,
    },
};
use driver_failure::test_support::{DRIVER_MESSAGE, fail_next_pipeline, render_pipeline};
use std::sync::Arc;

const OPTIONAL: &str = "finite depth-aware cloud pipeline";
const TERRAIN: &str = crate::chunk::pipeline::layouts::TERRAIN_PIPELINE_LABELS[0];

/// A noop device whose uncaptured errors are recorded, and a cache compiling on the caller.
fn cache(synchronous: bool) -> (wgpu::Device, Arc<Mutex<Vec<String>>>, PipelineCache) {
    let (device, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let uncaptured = Arc::new(Mutex::new(Vec::new()));
    let recorded = uncaptured.clone();
    device.on_uncaptured_error(Arc::new(move |error| {
        recorded.lock().unwrap().push(error.to_string());
    }));
    let mut cache = PipelineCache::new(RenderDevice::from(device.clone()), synchronous);
    cache.set_shader(
        Handle::<Shader>::default().id(),
        crate::shader_safety::from_wgsl(
            "@vertex fn vertex() -> @builtin(position) vec4f { return vec4f(0.0); }\n\
             @fragment fn fragment() -> @location(0) vec4f { return vec4f(1.0); }",
            "pipeline-failure-test.wgsl",
        ),
    );
    (device, uncaptured, cache)
}

/// Queues one labelled pipeline and runs the cache until that pipeline has an outcome.
fn compile(cache: &mut PipelineCache, label: &'static str) -> CachedRenderPipelineId {
    let shader = Handle::<Shader>::default();
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some(label.into()),
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("fragment".into()),
            targets: vec![Some(TextureFormat::Rgba8Unorm.into())],
            ..default()
        }),
        ..default()
    });
    settle(cache, id);
    id
}

/// Starts a queued pipeline and joins its compile, wherever the cache ran it.
fn settle(cache: &mut PipelineCache, id: CachedRenderPipelineId) {
    cache.process_queue();
    cache.block_on_render_pipeline(id);
    cache.process_queue();
}

fn driver_failed(cache: &PipelineCache, id: CachedRenderPipelineId) -> bool {
    matches!(
        cache.get_render_pipeline_state(id),
        CachedPipelineState::Err(ShaderCacheError::CreatePipeline(_))
    )
}

/// Reviews and applies the retries, as the render system does each frame.
fn review(failures: &mut PipelineFailures, cache: &mut PipelineCache) -> Vec<usize> {
    let retry = failures.review(cache, |_| false);
    for &id in &retry {
        assert!(cache.retry_failed_pipeline(id));
    }
    retry
}

#[test]
fn a_failed_optional_pipeline_disables_only_its_pass() {
    let (device, uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let clouds = compile(&mut cache, OPTIONAL);
    let terrain = compile(&mut cache, TERRAIN);
    assert!(driver_failed(&cache, clouds));

    let mut failures = PipelineFailures::default();
    for _ in 0..3 {
        assert!(review(&mut failures, &mut cache).is_empty());
        cache.process_queue();
    }
    assert!(cache.get_render_pipeline(clouds).is_none());
    assert!(driver_failed(&cache, clouds), "the pass must stay off");
    assert!(cache.get_render_pipeline(terrain).is_some());
    assert_eq!(failures.fatal(), None);
    assert!(uncaptured.lock().unwrap().is_empty());
}

#[test]
fn a_required_pipeline_that_fails_twice_stops_with_its_name_and_the_driver_report() {
    let (device, uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let terrain = compile(&mut cache, TERRAIN);
    let mut failures = PipelineFailures::default();
    assert_eq!(review(&mut failures, &mut cache), [terrain.id()]);
    assert_eq!(failures.fatal(), None);

    fail_next_pipeline(&device);
    settle(&mut cache, terrain);
    assert!(driver_failed(&cache, terrain));
    for _ in 0..3 {
        assert!(
            review(&mut failures, &mut cache).is_empty(),
            "one retry only"
        );
    }
    let fatal = failures.fatal().expect("the second failure is fatal");
    assert!(fatal.contains(&format!("\"{TERRAIN}\"")), "{fatal}");
    assert!(fatal.contains(DRIVER_MESSAGE), "{fatal}");
    assert!(!fatal.contains("Validation Error"), "{fatal}");
    assert!(uncaptured.lock().unwrap().is_empty());

    // The main world's GPU-failure path receives exactly that report.
    let mut render_world = World::new();
    let expected = fatal.to_owned();
    render_world.insert_resource(failures);
    render_world.init_resource::<MainWorld>();
    render_world.run_system_once(report_fatal_failure).unwrap();
    let main_world = render_world.resource::<MainWorld>();
    let failure = main_world.resource::<crate::GpuFailure>();
    assert_eq!(failure.message(), Some(expected.as_str()));
    assert!(!failure.is_device_lost());
}

#[test]
fn a_required_pipeline_recovers_when_its_retry_compiles() {
    let (device, _uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let ui = compile(&mut cache, crate::ui_render::pipeline::LABEL);
    let mut failures = PipelineFailures::default();
    assert_eq!(review(&mut failures, &mut cache), [ui.id()]);
    settle(&mut cache, ui);
    assert!(cache.get_render_pipeline(ui).is_some());
    assert!(review(&mut failures, &mut cache).is_empty());
    assert_eq!(failures.fatal(), None);
}

#[test]
fn a_view_output_blit_is_required_whatever_its_label() {
    let (device, _uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let blit = compile(&mut cache, "blit pipeline");
    let mut failures = PipelineFailures::default();
    assert_eq!(
        failures.review(&cache, |id| id == blit.id()),
        [blit.id()],
        "the pipeline a view presents with is retried"
    );
}

/// A view may only show up with the failed pipeline after the failure was first reviewed.
#[test]
fn a_failed_pipeline_a_view_adopts_later_is_still_retried() {
    let (device, _uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let blit = compile(&mut cache, "blit pipeline");
    let mut failures = PipelineFailures::default();
    assert!(failures.review(&cache, |_| false).is_empty());
    assert_eq!(failures.review(&cache, |id| id == blit.id()), [blit.id()]);
    assert!(
        failures.review(&cache, |id| id == blit.id()).is_empty(),
        "one retry only"
    );
}

/// The render system applies the retry itself, and a pipeline that recovered starts over.
#[test]
fn the_review_system_requeues_a_required_pipeline_and_forgets_it_once_it_compiles() {
    let (device, _uncaptured, mut cache) = cache(true);
    fail_next_pipeline(&device);
    let terrain = compile(&mut cache, TERRAIN);
    let mut world = World::new();
    world.insert_resource(cache);
    world.init_resource::<PipelineFailures>();
    world.run_system_once(review_failures).unwrap();
    assert!(matches!(
        world
            .resource::<PipelineCache>()
            .get_render_pipeline_state(terrain),
        CachedPipelineState::Queued
    ));
    settle(&mut world.resource_mut::<PipelineCache>(), terrain);
    world.run_system_once(review_failures).unwrap();
    let cache = world.resource::<PipelineCache>();
    assert!(cache.get_render_pipeline(terrain).is_some());
    let failures = world.resource::<PipelineFailures>();
    assert_eq!(failures.fatal(), None);
    assert!(failures.handled.is_empty());
}

/// The UI descriptors carry the labels this policy requires; terrain's descriptor tests check
/// theirs. Other pipelines, the blit included, are not required by label.
#[test]
fn the_ui_descriptors_are_required_pipelines() {
    use crate::ui_render::{composite, pipeline};
    let ui = pipeline::ui_pipeline_descriptor(pipeline::ui_bind_group_layout());
    assert!(required(
        ui.label.as_deref().expect("a labelled UI pipeline")
    ));
    assert!(required(composite::COMPOSITE_LABEL));
    assert!(required(composite::CLEAR_LABEL));
    assert!(!required(OPTIONAL));
    assert!(!required("blit pipeline"));
}

/// The cache's own worker captures the failure, so it never reaches the uncaptured handler.
#[test]
fn a_worker_compile_reports_the_failure_as_a_pipeline_state() {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let (device, uncaptured, mut cache) = cache(false);
    fail_next_pipeline(&device);
    let clouds = compile(&mut cache, OPTIONAL);
    let CachedPipelineState::Err(ShaderCacheError::CreatePipeline(description)) =
        cache.get_render_pipeline_state(clouds)
    else {
        panic!("the driver failure must be the pipeline's state");
    };
    assert!(description.contains(OPTIONAL), "{description}");
    assert!(cache.get_render_pipeline(clouds).is_none());
    assert!(uncaptured.lock().unwrap().is_empty());
}

#[test]
fn a_direct_optional_owner_stays_off_after_one_logged_failure() {
    let (device, uncaptured, _cache) = cache(true);
    fail_next_pipeline(&device);
    let label = "direct optional pass";
    assert!(optional(&device, label, || render_pipeline(&device, label)).is_none());
    assert!(uncaptured.lock().unwrap().is_empty());
    // The driver would build it now, but the pass is not attempted again.
    let mut attempts = 0;
    assert!(optional(&device, label, || attempts += 1).is_none());
    assert_eq!(attempts, 0);
    // Other passes and other devices are unaffected.
    assert!(optional(&device, "another pass", || render_pipeline(&device, label)).is_some());
    let (other, _queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    assert!(optional(&other, label, || render_pipeline(&other, label)).is_some());
}
