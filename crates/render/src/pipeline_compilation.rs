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
    /// A level without the driver cache also deletes what earlier launches saved.
    pub safe_graphics: render_api::SafeGraphics,
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
        if !self.safe_graphics.uses_driver_pipeline_cache() {
            // A cache the driver crashed on would be handed back to it at every launch.
            let directory = self.cache_dir.clone();
            std::thread::spawn(move || storage::discard(&directory));
            bevy::log::info!("driver pipeline cache discarded and unused: safe graphics");
        }
        let device = world.resource::<RenderDevice>().clone();
        let info = (**world.resource::<RenderAdapterInfo>()).clone();
        if !driver_cache_enabled(self.safe_graphics, info.backend, device.features()) {
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

/// Whether this launch reads and saves the driver's pipeline cache: only Vulkan devices have
/// one, and a safe graphics level may leave it unused.
fn driver_cache_enabled(
    safe_graphics: render_api::SafeGraphics,
    backend: wgpu::Backend,
    features: wgpu::Features,
) -> bool {
    safe_graphics.uses_driver_pipeline_cache()
        && backend == wgpu::Backend::Vulkan
        && features.contains(wgpu::Features::PIPELINE_CACHE)
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

#[cfg(test)]
mod tests {
    use super::*;
    use render_api::SafeGraphics;

    #[test]
    fn every_safe_graphics_level_leaves_the_driver_cache_unused() {
        let cache = wgpu::Features::PIPELINE_CACHE;
        assert!(driver_cache_enabled(
            SafeGraphics::Off,
            wgpu::Backend::Vulkan,
            cache
        ));
        for safe in SafeGraphics::ALL {
            assert_eq!(
                driver_cache_enabled(safe, wgpu::Backend::Vulkan, cache),
                !safe.is_active(),
                "{safe:?}"
            );
            assert!(!driver_cache_enabled(safe, wgpu::Backend::Metal, cache));
            assert!(!driver_cache_enabled(
                safe,
                wgpu::Backend::Vulkan,
                wgpu::Features::empty()
            ));
        }
    }
}
