//! The main-world load lifecycle: starting a worker for each request, cancelling superseded
//! ones and publishing results.

use super::loader::{PackGpu, load};
use super::pipelines::GpuContext;
use super::plugin::{JAVA_DIMENSIONS, LoadedShaderPack};
use super::resources::vanilla::VanillaTextures;
use bevy::{
    prelude::*,
    render::renderer::{RenderAdapter, RenderDevice, RenderQueue},
};
use render_scene::shaderpack::{
    ShaderPackFrameInputs, ShaderPackLoadProgress, ShaderPackRequest, ShaderPackSettledRevision,
    ShaderPackStatus,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};

type LoadResult = Result<PackGpu, String>;

/// Pack path, option-file path, dimension and revision of one load.
type LoaderKey = (PathBuf, Option<PathBuf>, i32, u64);

/// The pack being loaded or shown, including the option file it reads.
#[derive(Resource, Default)]
pub(super) struct Loader {
    pub(super) key: Option<LoaderKey>,
    pub(super) job: Option<Mutex<mpsc::Receiver<LoadResult>>>,
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
) {
    let wanted = loader_key(&request, inputs.dimension);
    if loader.job.is_some() && wanted != loader.key {
        // The superseded worker is cancelled and left to stop on its own, so a long driver
        // compile in it never holds up the next selection.
        progress.finish();
        loader.job = None;
        loader.key = None;
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
            Ok(_) => loader.job = Some(Mutex::new(receive)),
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
    let Some(job) = loader.job.as_mut() else {
        return;
    };
    let outcome = match job.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv() {
        Ok(outcome) => outcome,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err("the load worker stopped".to_owned()),
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
