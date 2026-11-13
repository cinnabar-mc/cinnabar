//! Names each pipeline in the session log while the driver compiles it.
//!
//! A driver that faults inside pipeline creation kills the process from native code, and
//! queued log output dies with it. The application therefore supplies a writer whose lines
//! reach the log file before it returns, and every compile writes a `started` line before the
//! driver call and a `finished` line after it. The last `started` lines without a `finished`
//! line name what the driver was compiling.
//!
//! Pipelines the renderer creates itself go through [`CompilePipelines`], which brackets the
//! driver call on the compiling thread. Bevy's `PipelineCache` compiles on its own workers and
//! reports nothing, so its pipelines are logged from the render thread: `started` as soon as a
//! compile has been handed to a worker, and `finished` once its result is collected. Several
//! of those can be open at once, and each `finished` line may trail its compile by a frame.

use bevy::{
    prelude::{App, IntoScheduleConfigs, Local, Plugin, ResMut},
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor},
        renderer::{RenderDevice, render_system},
    },
    shader::ShaderCacheError,
};
use std::sync::OnceLock;

/// Writes one line to the session log, reaching the file before it returns.
pub type BreadcrumbWriter = fn(&str);

static WRITER: OnceLock<BreadcrumbWriter> = OnceLock::new();

/// Logs every pipeline compile through `writer`. Without this plugin compiles are not logged.
pub struct PipelineBreadcrumbPlugin {
    pub writer: BreadcrumbWriter,
}

impl Plugin for PipelineBreadcrumbPlugin {
    /// The first writer installed in a process serves all its renderers.
    fn build(&self, _app: &mut App) {
        let _ = WRITER.set(self.writer);
    }

    /// Watches Bevy's cache once the renderer has created it.
    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        if render.world().contains_resource::<PipelineCache>() {
            render.add_systems(
                Render,
                log_cached_compiles
                    .in_set(RenderSystems::Render)
                    .before(render_system),
            );
        }
    }
}

/// What a pipeline is, as its log lines name it.
#[derive(Clone, Copy)]
enum Kind {
    Render,
    Compute,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Compute => "compute",
        }
    }
}

fn started(kind: Kind, label: &str) -> String {
    format!("Pipeline compile started: {} {label:?}", kind.name())
}

fn finished(kind: Kind, label: &str) -> String {
    format!("Pipeline compile finished: {} {label:?}", kind.name())
}

/// The installed writer, if any.
fn writer() -> Option<&'static dyn Fn(&str)> {
    WRITER.get().map(|writer| writer as &dyn Fn(&str))
}

/// Runs `compile`, the driver call for the pipeline `label`, between its two log lines.
fn bracket<T>(
    writer: Option<&dyn Fn(&str)>,
    kind: Kind,
    label: Option<&str>,
    compile: impl FnOnce() -> T,
) -> T {
    let Some(writer) = writer else {
        return compile();
    };
    let label = label.unwrap_or("unlabelled");
    writer(&started(kind, label));
    let pipeline = compile();
    writer(&finished(kind, label));
    pipeline
}

/// Pipeline creation that leaves the pipeline's label in the session log while the driver
/// compiles it. Renderer code creates its own pipelines through these methods, not the
/// device's `create_*_pipeline`.
pub(crate) trait CompilePipelines {
    type Render;
    type Compute;

    /// Creates a render pipeline between its `started` and `finished` log lines.
    fn compile_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Self::Render;

    /// Creates a compute pipeline between its `started` and `finished` log lines.
    fn compile_compute_pipeline(
        &self,
        descriptor: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> Self::Compute;
}

impl CompilePipelines for wgpu::Device {
    type Render = wgpu::RenderPipeline;
    type Compute = wgpu::ComputePipeline;

    fn compile_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Self::Render {
        bracket(writer(), Kind::Render, descriptor.label, || {
            self.create_render_pipeline(descriptor)
        })
    }

    fn compile_compute_pipeline(
        &self,
        descriptor: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> Self::Compute {
        bracket(writer(), Kind::Compute, descriptor.label, || {
            self.create_compute_pipeline(descriptor)
        })
    }
}

impl CompilePipelines for RenderDevice {
    type Render = bevy::render::render_resource::RenderPipeline;
    type Compute = bevy::render::render_resource::ComputePipeline;

    fn compile_render_pipeline(
        &self,
        descriptor: &wgpu::RenderPipelineDescriptor<'_>,
    ) -> Self::Render {
        bracket(writer(), Kind::Render, descriptor.label, || {
            self.create_render_pipeline(descriptor)
        })
    }

    fn compile_compute_pipeline(
        &self,
        descriptor: &wgpu::ComputePipelineDescriptor<'_>,
    ) -> Self::Compute {
        bracket(writer(), Kind::Compute, descriptor.label, || {
            self.create_compute_pipeline(descriptor)
        })
    }
}

/// Where a cached pipeline is in Bevy's queue.
enum Stage {
    /// Waiting for a worker's result, or for a shader that has not loaded yet.
    Pending,
    Ready,
    /// A failure Bevy does not retry, with its message.
    Failed(String),
}

/// Follows Bevy's cached pipelines by their index in the cache, which never changes.
#[derive(Default)]
struct CachedCompiles {
    /// Pipelines below this index have had their `started` line.
    next: usize,
    /// Started pipelines without a result, with their names.
    open: Vec<(usize, Kind, String)>,
}

impl CachedCompiles {
    /// Writes a `started` line for each pipeline first seen among `count`, then the result of
    /// every open pipeline that has one. `stage` and `name` read a pipeline by index.
    fn advance(
        &mut self,
        count: usize,
        stage: impl Fn(usize) -> Stage,
        name: impl Fn(usize) -> (Kind, String),
        mut write: impl FnMut(&str),
    ) {
        if count < self.next {
            // A rebuilt renderer starts a new cache.
            *self = Self::default();
        }
        for index in self.next..count {
            let (kind, label) = name(index);
            write(&started(kind, &label));
            self.open.push((index, kind, label));
        }
        self.next = count;
        self.open
            .retain(|(index, kind, label)| match stage(*index) {
                Stage::Pending => true,
                Stage::Ready => {
                    write(&finished(*kind, label));
                    false
                }
                Stage::Failed(error) => {
                    write(&format!(
                        "Pipeline compile failed: {} {label:?}: {error}",
                        kind.name()
                    ));
                    false
                }
            });
    }
}

/// The label of a cached pipeline, or the shader it starts from when it has none.
fn cached_name(descriptor: &PipelineDescriptor) -> (Kind, String) {
    let (kind, label, shader) = match descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
            (Kind::Render, &descriptor.label, &descriptor.vertex.shader)
        }
        PipelineDescriptor::ComputePipelineDescriptor(descriptor) => {
            (Kind::Compute, &descriptor.label, &descriptor.shader)
        }
        PipelineDescriptor::MeshPipelineDescriptor(descriptor) => {
            (Kind::Render, &descriptor.label, &descriptor.mesh.shader)
        }
    };
    let name = match (label, shader.path()) {
        (Some(label), _) => label.to_string(),
        (None, Some(path)) => path.to_string(),
        (None, None) => "unlabelled".to_owned(),
    };
    (kind, name)
}

fn cached_stage(state: &CachedPipelineState) -> Stage {
    match state {
        CachedPipelineState::Ok(_) => Stage::Ready,
        CachedPipelineState::Queued
        | CachedPipelineState::Creating(_)
        | CachedPipelineState::Err(
            ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable,
        ) => Stage::Pending,
        CachedPipelineState::Err(error) => Stage::Failed(error.to_string()),
    }
}

/// Starts this frame's queued compiles, so their `started` lines are written before the frame
/// renders, and logs what the cache did. Bevy's own queue pass this frame then finds nothing
/// new to start.
fn log_cached_compiles(mut cache: ResMut<PipelineCache>, mut compiles: Local<CachedCompiles>) {
    let Some(writer) = WRITER.get() else {
        return;
    };
    cache.process_queue();
    let count = cache.pipelines().count();
    if count == compiles.next && compiles.open.is_empty() {
        return;
    }
    let pipeline = |index| cache.pipelines().nth(index);
    compiles.advance(
        count,
        |index| pipeline(index).map_or(Stage::Pending, |pipeline| cached_stage(&pipeline.state)),
        |index| {
            pipeline(index).map_or((Kind::Render, "unlabelled".to_owned()), |pipeline| {
                cached_name(&pipeline.descriptor)
            })
        },
        writer,
    );
}

// The integration tests include this file by path, where a bare `mod tests` resolves elsewhere.
#[cfg(test)]
#[path = "pipeline_breadcrumbs/tests.rs"]
mod tests;
