//! Configures worker compilation and persists Vulkan's optional driver cache off the frame.

use bevy::{
    prelude::{App, IntoScheduleConfigs, Plugin, ResMut, Resource, warn},
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::PipelineCache,
        renderer::{RenderAdapterInfo, RenderDevice},
    },
};
use std::{path::PathBuf, thread::JoinHandle};

mod storage;

/// Enables background compilation and optional Vulkan cache reuse for the client renderer.
pub struct PipelineCompilationPlugin {
    pub cache_dir: PathBuf,
}

impl Plugin for PipelineCompilationPlugin {
    /// Starts optional disk and driver work only after Bevy has installed the device.
    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let world = render.world_mut();
        let Some(mut cache) = world.get_resource_mut::<PipelineCache>() else {
            return;
        };
        cache.set_async_pipeline_compilation_on_macos(true);
        let device = world.resource::<RenderDevice>().clone();
        let info = (**world.resource::<RenderAdapterInfo>()).clone();
        if info.backend != wgpu::Backend::Vulkan
            || !device.features().contains(wgpu::Features::PIPELINE_CACHE)
        {
            return;
        }
        let directory = self.cache_dir.clone();
        world.insert_resource(CachePersistence {
            loading: Some(std::thread::spawn(move || {
                storage::DriverCache::open(&device, &info, directory)
            })),
            cache: None,
            saving: None,
            saved_pipelines: 0,
        });
        render.add_systems(Render, persist.before(RenderSystems::ExtractCommands));
    }

    /// The renderer is configured during `finish`, after device initialization.
    fn build(&self, _app: &mut App) {}
}

#[derive(Resource)]
struct CachePersistence {
    loading: Option<JoinHandle<Option<storage::DriverCache>>>,
    cache: Option<storage::DriverCache>,
    saving: Option<JoinHandle<()>>,
    saved_pipelines: usize,
}

/// Polls finite workers without waiting and stores only after a changed pipeline set has finished.
fn persist(mut state: ResMut<CachePersistence>, mut pipelines: ResMut<PipelineCache>) {
    if state.loading.as_ref().is_some_and(JoinHandle::is_finished) {
        match state.loading.take().expect("finished cache load").join() {
            Ok(cache) => {
                pipelines.set_pipeline_cache(cache.as_ref().map(|cache| cache.pipeline.clone()));
                state.cache = cache;
            }
            Err(_) => warn!("optional driver cache worker failed"),
        }
    }
    if state.saving.as_ref().is_some_and(JoinHandle::is_finished)
        && state
            .saving
            .take()
            .expect("finished cache save")
            .join()
            .is_err()
    {
        warn!("optional driver cache save worker failed");
    }
    if state.saving.is_some() || pipelines.waiting_pipelines().next().is_some() {
        return;
    }
    let count = pipelines.pipelines().count();
    if count == state.saved_pipelines {
        return;
    }
    if let Some(cache) = state.cache.clone() {
        state.saved_pipelines = count;
        state.saving = Some(std::thread::spawn(move || cache.store()));
    }
}
