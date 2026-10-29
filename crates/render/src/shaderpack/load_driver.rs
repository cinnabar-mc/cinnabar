//! The main-world load lifecycle: starting a worker for each request, retiring superseded
//! ones and publishing results, including a worker that stops making progress.

use super::loader::{PackGpu, load};
use super::pipelines::GpuContext;
use super::plugin::{JAVA_DIMENSIONS, LoadedShaderPack};
use super::resources::vanilla::VanillaTextures;
use bevy::{
    prelude::*,
    render::renderer::{RenderAdapter, RenderDevice, RenderQueue},
};
use render_scene::shaderpack::{
    LoadProgress, ShaderPackFrameInputs, ShaderPackLoadProgress, ShaderPackRequest,
    ShaderPackSettledRevision, ShaderPackStatus,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// How long a load may go without recording progress before it fails and Vanilla draws; above
/// the compiler's own limit, so only a step that never returns, such as a driver compile, trips it.
pub(super) const STALL_LIMIT: Duration = Duration::from_secs(180);
const _: () = assert!(STALL_LIMIT.as_secs() > shaderpack::worker::TIME_LIMIT.as_secs());

type LoadResult = Result<PackGpu, String>;

/// Pack path, option-file path, dimension and revision of one load.
type LoaderKey = (PathBuf, Option<PathBuf>, i32, u64);

/// The pack being loaded or shown, including the option file it reads.
#[derive(Resource, Default)]
pub(super) struct Loader {
    pub(super) key: Option<LoaderKey>,
    pub(super) job: Option<Job>,
    pub(super) retiring: bool,
}

/// One load worker's result channel and the progress it records.
pub(super) struct Job {
    result: Mutex<mpsc::Receiver<LoadResult>>,
    progress: Arc<LoadProgress>,
    /// The worker's step count and when it was first seen.
    seen: Option<(u64, Duration)>,
}

impl Job {
    /// Watches `result` from the worker that records into `progress`.
    pub(super) fn new(result: mpsc::Receiver<LoadResult>, progress: Arc<LoadProgress>) -> Self {
        Self {
            result: Mutex::new(result),
            progress,
            seen: None,
        }
    }

    /// The worker's outcome at `now`, or an error once it has recorded no progress for
    /// [`STALL_LIMIT`]; a stalled worker is left to finish on its own, cancelled.
    fn poll(&mut self, now: Duration) -> Option<LoadResult> {
        match self
            .result
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
        {
            Ok(outcome) => return Some(outcome),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Some(Err("the load worker stopped".to_owned()));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let steps = self.progress.steps();
        let since = match self.seen {
            Some((seen, since)) if seen == steps => since,
            _ => {
                self.seen = Some((steps, now));
                return None;
            }
        };
        (now.saturating_sub(since) >= STALL_LIMIT).then(|| {
            let step = self.progress.step();
            Err(format!(
                "stopped responding while {}: no progress for {} s",
                if step.is_empty() { "loading" } else { &step },
                STALL_LIMIT.as_secs()
            ))
        })
    }
}

/// Identifies the request whose load can be reused in the current dimension.
pub(super) fn loader_key(request: &ShaderPackRequest, dimension: i32) -> Option<LoaderKey> {
    request.pack.clone().map(|path| {
        (
            path,
            request.options.clone(),
            dimension.clamp(0, 2),
            request.revision,
        )
    })
}

/// The render device, queue and adapter Bevy also publishes in the main world.
type MainWorldGpu<'w> = (
    Option<Res<'w, RenderDevice>>,
    Option<Res<'w, RenderQueue>>,
    Option<Res<'w, RenderAdapter>>,
);

/// Starts a load when the requested pack, its revision or the dimension changes and
/// publishes results. A dimension change or reload of the shown pack keeps it on screen
/// until the new load is ready. The pack's `minecraft:` texture references resolve against
/// the vanilla textures loaded when its load starts.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading each resource the load starts from and publishes to"
)]
pub(super) fn drive_loader(
    request: Res<ShaderPackRequest>,
    inputs: Res<ShaderPackFrameInputs>,
    mut status: ResMut<ShaderPackStatus>,
    mut warnings: Option<ResMut<render_scene::shaderpack::ShaderPackWarnings>>,
    mut settled: ResMut<ShaderPackSettledRevision>,
    mut loaded: ResMut<LoadedShaderPack>,
    mut progress: ResMut<ShaderPackLoadProgress>,
    mut loader: ResMut<Loader>,
    gpu: MainWorldGpu,
    atmosphere: Option<Res<render_scene::AtmosphereTextureAssets>>,
    compiler: Option<Res<render_scene::shaderpack::ShaderPackCompilerExecutable>>,
    time: Res<Time<Real>>,
) {
    let now = time.elapsed();
    let wanted = loader_key(&request, inputs.dimension);
    if loader.job.is_some() && (wanted != loader.key || loader.retiring) {
        progress.finish();
        loader.retiring = true;
        let keep_shown = wanted.as_ref().is_some_and(
            |(path, ..)| matches!(&*status, ShaderPackStatus::Ready(shown) if shown == path),
        );
        if !keep_shown {
            loaded.0 = None;
            status.set_if_neq(
                wanted
                    .as_ref()
                    .map_or(ShaderPackStatus::Idle, |(path, ..)| {
                        ShaderPackStatus::Loading(path.clone())
                    }),
            );
        }
        // Keep the receiver until the old worker exits. Rapid selections coalesce into the
        // latest request rather than accumulating detached CPU and GPU load workers; only a
        // stalled worker is left behind.
        match loader.job.as_mut().and_then(|job| job.poll(now)) {
            None => return,
            Some(Err(reason)) => debug!(
                target: "cinnabar::shaderpack",
                "superseded shader pack load ended: {reason}"
            ),
            Some(Ok(_)) => {}
        }
        loader.job = None;
        loader.key = None;
        loader.retiring = false;
    }
    if wanted != loader.key {
        let Some((path, _, dimension, _)) = wanted.clone() else {
            loader.key = None;
            loader.job = None;
            progress.finish();
            status.set_if_neq(ShaderPackStatus::Idle);
            loaded.0 = None;
            return;
        };
        let (Some(device), Some(queue), Some(adapter)) = gpu else {
            return;
        };
        loader.key = wanted;
        loader.job = None;
        if let Err(reason) = super::kernels::validate_device(&device.limits()) {
            progress.finish();
            loaded.0 = None;
            settled.0 = request.revision;
            *status = ShaderPackStatus::Failed { pack: path, reason };
            return;
        }
        let gpu = GpuContext {
            device: device.wgpu_device().clone(),
            queue: (**queue).clone(),
            adapter: (**adapter).clone(),
        };
        let reloading = matches!(&*status, ShaderPackStatus::Ready(shown) if *shown == path);
        if !reloading {
            status.set_if_neq(ShaderPackStatus::Loading(path.clone()));
            loaded.0 = None;
        }
        let cache_dir = request.cache_dir.clone();
        let options = request.options.clone();
        let compiler = compiler.as_deref().map(|compiler| compiler.0.clone());
        let vanilla = atmosphere
            .as_deref()
            .map(VanillaTextures::from_atmosphere)
            .unwrap_or_default();
        let (send, receive) = mpsc::channel();
        let worker_progress = progress.start();
        let watched = Arc::clone(&worker_progress);
        let spawned = std::thread::Builder::new()
            .name("shader pack load".into())
            .spawn(move || {
                let id = JAVA_DIMENSIONS[dimension as usize];
                let outcome = load(
                    &path,
                    options.as_deref(),
                    id,
                    cache_dir.as_deref(),
                    &gpu,
                    &vanilla,
                    &worker_progress,
                    compiler.as_deref(),
                );
                let _ = send.send(outcome);
            });
        match spawned {
            Ok(_) => loader.job = Some(Job::new(receive, watched)),
            Err(error) => {
                progress.finish();
                if let Some((path, _, _, revision)) = &loader.key {
                    *status = ShaderPackStatus::Failed {
                        pack: path.clone(),
                        reason: error.to_string(),
                    };
                    settled.0 = *revision;
                }
            }
        }
        return;
    }
    let Some(outcome) = loader.job.as_mut().and_then(|job| job.poll(now)) else {
        return;
    };
    loader.job = None;
    progress.finish();
    let Some((path, _, _, revision)) = loader.key.clone() else {
        return;
    };
    settled.0 = revision;
    match outcome {
        Ok(pack) => {
            if let Some(warnings) = warnings.as_deref_mut() {
                warnings.0.clone_from(&pack.warnings);
            }
            loaded.0 = Some(Arc::new(pack));
            *status = ShaderPackStatus::Ready(path);
        }
        Err(reason) => {
            warn!(
                target: "cinnabar::shaderpack",
                pack = %path.display(),
                "shader pack unavailable, rendering Vanilla: {reason}"
            );
            loaded.0 = None;
            *status = ShaderPackStatus::Failed { pack: path, reason };
        }
    }
}

#[cfg(test)]
mod tests;
