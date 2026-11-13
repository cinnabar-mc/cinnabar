//! What a pipeline the graphics driver failed to build costs the session. A pass the game works
//! without stays off. A pipeline the client cannot draw a usable frame without is compiled once
//! more, and a second failure stops the client with the pipeline's name and the driver's report.
//!
//! Uncaptured GPU errors still stop rendering through [`crate::gpu_error_policy`]; everything
//! here runs before a failure could become one.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock, PoisonError};

use bevy::{
    core_pipeline::upscaling::ViewUpscalingPipeline,
    prelude::{App, IntoScheduleConfigs, Query, Res, ResMut, Resource, error, warn},
    render::{
        ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems,
        error_handler::ErrorType,
        render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor},
    },
    shader::ShaderCacheError,
};

use crate::gpu_allocation::DeviceKey;

/// Whether `label` names a cached pipeline the client stops without: the terrain variants and
/// the retained UI with its composite. Every label is its owner's constant, so a renamed
/// pipeline cannot leave this policy silently, and tests check the descriptors those owners
/// build against it.
pub(crate) fn required(label: &str) -> bool {
    crate::chunk::pipeline::layouts::TERRAIN_PIPELINE_LABELS.contains(&label)
        || [
            crate::ui_render::pipeline::LABEL,
            crate::ui_render::composite::COMPOSITE_LABEL,
            crate::ui_render::composite::CLEAR_LABEL,
        ]
        .contains(&label)
}

/// Runs the direct shader-module and pipeline creation of a pass the game works without.
///
/// Returns `None` when the driver failed to build any of it. The failure is logged once with
/// `label`, and every later call for the same device and label returns `None` without asking
/// the driver again, so the pass stays off for the session instead of recompiling each frame.
/// The caller treats `None` as it treats a failed allocation. `create` runs on the calling
/// thread under [`driver_failure::catch`]'s contract.
pub(crate) fn optional<T>(
    device: &wgpu::Device,
    label: &str,
    create: impl FnOnce() -> T,
) -> Option<T> {
    static FAILED: OnceLock<Mutex<HashSet<(DeviceKey, String)>>> = OnceLock::new();
    let failed = FAILED.get_or_init(Default::default);
    let key = (
        crate::gpu_allocation::wgpu_device_key(device),
        label.to_owned(),
    );
    if failed
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&key)
    {
        return None;
    }
    match driver_failure::catch(device, create) {
        Ok(created) => Some(created),
        Err(failure) => {
            error!(
                target: "cinnabar::render",
                label, "The graphics driver could not build this pipeline; its pass stays off: {failure}"
            );
            failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key);
            None
        }
    }
}

/// Turns a direct owner's driver failure into the absent owner, logged with `label`. For
/// owners built once, or that disable themselves, so the driver is not asked again.
pub(crate) fn unavailable<T>(
    label: &str,
    built: Result<T, driver_failure::DriverFailure>,
) -> Option<T> {
    built
        .inspect_err(|failure| {
            error!(
                target: "cinnabar::render",
                label,
                "The graphics driver could not build this pipeline; its pass stays off: {failure}"
            );
        })
        .ok()
}

/// How far the policy has taken one failed pipeline.
enum Handled {
    /// Logged as an optional pass that stays off.
    Optional,
    /// A required pipeline is compiling a second time.
    Retried,
    /// A required pipeline failed twice and stopped the client.
    Fatal,
}

/// The cached pipelines the driver failed to build, and the failure that stops the client.
#[derive(Resource, Default)]
pub(crate) struct PipelineFailures {
    handled: HashMap<usize, Handled>,
    fatal: Option<String>,
}

impl PipelineFailures {
    /// The report for the first required pipeline that failed twice.
    pub(crate) fn fatal(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    /// Applies the policy to every cached pipeline that ended in a driver failure and returns
    /// the ones to compile again. `presents` tells whether a pipeline is a view's output blit,
    /// which is required whatever its label. Each failure is logged when its handling changes,
    /// and a pipeline that compiled after all is forgotten.
    fn review(&mut self, cache: &PipelineCache, presents: impl Fn(usize) -> bool) -> Vec<usize> {
        let mut retry = Vec::new();
        for (id, cached) in cache.pipelines().enumerate() {
            let CachedPipelineState::Err(ShaderCacheError::CreatePipeline(description)) =
                &cached.state
            else {
                continue;
            };
            let label = label(&cached.descriptor);
            let report = driver_failure::report(description);
            match self.handled.get(&id) {
                Some(Handled::Fatal) => {}
                Some(Handled::Retried) => {
                    self.handled.insert(id, Handled::Fatal);
                    let message = format!(
                        "the graphics driver could not build the \"{label}\" pipeline, which \
                         the game cannot run without, in two attempts. Driver report: {report}"
                    );
                    error!(target: "cinnabar::render", "{message}");
                    self.fatal.get_or_insert(message);
                }
                // A view may adopt a failed pipeline as its output after it was logged.
                None | Some(Handled::Optional) if required(label) || presents(id) => {
                    warn!(
                        target: "cinnabar::render",
                        label,
                        "The graphics driver could not build a required pipeline; trying once more: {report}"
                    );
                    self.handled.insert(id, Handled::Retried);
                    retry.push(id);
                }
                Some(Handled::Optional) => {}
                None => {
                    error!(
                        target: "cinnabar::render",
                        label,
                        "The graphics driver could not build this pipeline; its pass stays off: {report}"
                    );
                    self.handled.insert(id, Handled::Optional);
                }
            }
        }
        // Failures are rare, so this walk is normally empty. A retry in flight keeps its record.
        self.handled.retain(|&id, _| {
            cache
                .pipelines()
                .nth(id)
                .is_some_and(|cached| !matches!(cached.state, CachedPipelineState::Ok(_)))
        });
        retry
    }
}

/// The debug label a pipeline was queued with.
fn label(descriptor: &PipelineDescriptor) -> &str {
    match descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(descriptor) => descriptor.label.as_deref(),
        PipelineDescriptor::ComputePipelineDescriptor(descriptor) => descriptor.label.as_deref(),
        PipelineDescriptor::MeshPipelineDescriptor(descriptor) => descriptor.label.as_deref(),
    }
    .unwrap_or("unlabelled")
}

/// Reviews the cache after the frame's compiles were collected and queues the allowed retries.
fn review_failures(
    cache: Option<ResMut<PipelineCache>>,
    blits: Query<&ViewUpscalingPipeline>,
    mut failures: ResMut<PipelineFailures>,
) {
    let Some(mut cache) = cache else {
        return;
    };
    let retry = failures.review(&cache, |id| {
        blits.iter().any(|blit| blit.pipeline_id().id() == id)
    });
    for id in retry {
        cache.retry_failed_pipeline(id);
    }
}

/// Hands a required pipeline's failure to the main world, whose GPU-failure path explains it
/// and ends the session.
fn report_fatal_failure(failures: Res<PipelineFailures>, mut main_world: ResMut<MainWorld>) {
    if let Some(message) = failures.fatal() {
        main_world
            .get_resource_or_init::<crate::GpuFailure>()
            .record(ErrorType::Internal, message);
    }
}

/// Watches the render world's pipeline cache; a no-op for an app without a renderer.
pub(crate) fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<PipelineFailures>()
        .add_systems(ExtractSchedule, report_fatal_failure)
        .add_systems(Render, review_failures.in_set(RenderSystems::Cleanup));
}

#[cfg(test)]
#[path = "pipeline_failure/tests.rs"]
mod tests;
