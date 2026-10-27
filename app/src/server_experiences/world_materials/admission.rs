//! Admitting one declaration off the frame: validating its shader and reading its textures'
//! sizes, then, once its charge fits, decoding its textures. Each step runs on the async
//! compute pool and the presenter picks up its result on a later frame.

use super::MAX_TEXTURE_EXTENT;
use mod_render::world_material::{
    self as abi,
    publication::{Blend, Depth, Published, Texture},
};
use server_experience::{
    screen::Files,
    world_material::{WorldBlend, WorldDepth, WorldMaterialSpec},
};
#[cfg(test)]
use std::sync::Mutex;
use std::{
    io::Cursor,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// One unit of admission work a test runs by hand.
#[cfg(test)]
pub(super) type Job = Box<dyn FnOnce() + Send>;

/// Where admission work runs.
#[derive(Clone, Default)]
pub(super) enum Executor {
    /// Bevy's async compute pool, off the frame.
    #[default]
    Pool,
    /// On the calling thread, so a test's rebuild settles at once.
    #[cfg(test)]
    Inline,
    /// Kept until a test runs it, so a test can observe work still pending.
    #[cfg(test)]
    Held(Arc<Mutex<Vec<Job>>>),
}

impl Executor {
    /// Runs `job` here or hands it on; `done` counts it when it finishes.
    fn run(&self, job: impl FnOnce() + Send + 'static, done: &Arc<AtomicU64>) {
        let done = Arc::clone(done);
        let job = move || {
            job();
            done.fetch_add(1, Ordering::Release);
        };
        match self {
            Executor::Pool => bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default)
                .spawn(async move { job() })
                .detach(),
            #[cfg(test)]
            Executor::Inline => job(),
            #[cfg(test)]
            Executor::Held(held) => held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(Box::new(job)),
        }
    }
}

/// A result a job fills in once.
type Slot<T> = Arc<OnceLock<T>>;

/// What admitting a declaration has produced so far.
pub(super) enum Outcome {
    /// Validating its shader and reading its textures' sizes.
    Measuring(Slot<Result<Measured, String>>),
    /// A valid shader whose charge did not fit its bundle's remaining shares when last tried.
    /// It holds no decoded texels.
    Waiting(Measured),
    /// Decoding its textures, its charge reserved.
    Decoding(Measured, Slot<Result<Arc<[Texture]>, String>>),
    /// Drawable, with its decoded textures.
    Ready(Published),
    /// Refused for this reason until its program changes.
    Refused(String),
}

/// A validated shader and the bytes its textures decode to.
#[derive(Clone)]
pub(super) struct Measured {
    material: Arc<abi::WorldMaterial>,
    texels: u64,
}

/// What one material costs its bundle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Charge {
    pub gpu: u64,
    pub memory: u64,
}

/// What a material's pipelines are compiled for: Bevy keeps one set per distinct identity.
pub(super) type Pipelines = (u128, Blend, Depth);

/// How fitting a material against its bundle's shares went.
pub(super) enum Fit<'a> {
    Drawable(&'a Published, Charge),
    /// Still being measured or decoded; not a failure.
    Pending(Charge),
    Refused(String),
}

/// A declaration's last outcome, kept while its program is unchanged.
pub(super) struct Cached {
    pub spec: WorldMaterialSpec,
    /// Numbers this admission attempt, so a failure is reported once per attempt; it is also
    /// the material's key.
    pub attempt: u64,
    pub outcome: Outcome,
}

impl Cached {
    /// Starts admitting `spec` as attempt `attempt`: its shader is validated and its textures'
    /// headers read by `executor`, so the frame never waits for either.
    pub(super) fn admit(
        spec: &WorldMaterialSpec,
        attempt: u64,
        files: &Arc<Files>,
        executor: &Executor,
        done: &Arc<AtomicU64>,
    ) -> Self {
        let slot: Slot<Result<Measured, String>> = Arc::default();
        let (result, source, textures, files) = (
            Arc::clone(&slot),
            spec.source.clone(),
            spec.textures.clone(),
            Arc::clone(files),
        );
        executor.run(
            move || {
                let _ = result.set(measure(&source, &textures, &files));
            },
            done,
        );
        Self {
            spec: spec.clone(),
            attempt,
            outcome: Outcome::Measuring(slot),
        }
    }

    /// The identity of the pipelines this material compiles, once measured.
    pub(super) fn pipelines(&mut self) -> Option<Pipelines> {
        self.settle();
        let material = match &self.outcome {
            Outcome::Waiting(measured) | Outcome::Decoding(measured, _) => &measured.material,
            Outcome::Ready(published) => &published.material,
            Outcome::Measuring(_) | Outcome::Refused(_) => return None,
        };
        Some((
            material.program(),
            blend(self.spec.blend),
            depth(self.spec.depth),
        ))
    }

    /// Charges the material against the `gpu` and `memory` bytes its bundle has left, where
    /// `pipeline` is what its pipelines cost (nothing when the bundle already compiled them),
    /// and starts decoding its textures once the charge fits. A material that no longer fits
    /// gives up its decoded textures, so the host never holds texels its shares do not cover.
    pub(super) fn fit(
        &mut self,
        gpu: u64,
        memory: u64,
        pipeline: u64,
        files: &Arc<Files>,
        executor: &Executor,
        done: &Arc<AtomicU64>,
    ) -> Fit<'_> {
        self.settle();
        let measured = match &self.outcome {
            Outcome::Measuring(_) => {
                return Fit::Pending(Charge { gpu: 0, memory: 0 });
            }
            Outcome::Refused(reason) => return Fit::Refused(reason.clone()),
            Outcome::Waiting(measured) | Outcome::Decoding(measured, _) => measured.clone(),
            Outcome::Ready(published) => Measured {
                material: Arc::clone(&published.material),
                texels: published.textures.iter().map(Texture::bytes).sum(),
            },
        };
        let charge = Charge {
            gpu: measured.texels.saturating_add(pipeline),
            memory: (measured.material.source.len() as u64).saturating_add(measured.texels),
        };
        let refusal = if charge.gpu > gpu {
            Some("exceeds the bundle's GPU limit")
        } else if charge.memory > memory {
            Some("exceeds the bundle's memory limit")
        } else {
            None
        };
        if let Some(reason) = refusal {
            self.outcome = Outcome::Waiting(measured);
            return Fit::Refused(reason.into());
        }
        if let Outcome::Waiting(measured) = &self.outcome {
            let slot: Slot<Result<Arc<[Texture]>, String>> = Arc::default();
            let (result, textures, files) = (
                Arc::clone(&slot),
                self.spec.textures.clone(),
                Arc::clone(files),
            );
            executor.run(
                move || {
                    let _ = result.set(decode_all(&textures, &files).map(Arc::from));
                },
                done,
            );
            self.outcome = Outcome::Decoding(measured.clone(), slot);
            self.settle();
        }
        match &self.outcome {
            Outcome::Ready(published) => Fit::Drawable(published, charge),
            Outcome::Refused(reason) => Fit::Refused(reason.clone()),
            _ => Fit::Pending(charge),
        }
    }

    /// Takes any finished job's result.
    fn settle(&mut self) {
        let next = match &self.outcome {
            Outcome::Measuring(slot) => match slot.get() {
                Some(Ok(measured)) => Outcome::Waiting(measured.clone()),
                Some(Err(reason)) => Outcome::Refused(reason.clone()),
                None => return,
            },
            Outcome::Decoding(measured, slot) => match slot.get() {
                Some(Ok(textures)) => Outcome::Ready(Published {
                    key: self.attempt,
                    material: Arc::clone(&measured.material),
                    textures: Arc::clone(textures),
                    depth: depth(self.spec.depth),
                    blend: blend(self.spec.blend),
                }),
                Some(Err(reason)) => Outcome::Refused(reason.clone()),
                None => return,
            },
            _ => return,
        };
        self.outcome = next;
    }

    /// Bytes of decoded texels this attempt holds.
    #[cfg(test)]
    pub(super) fn held_texels(&self) -> u64 {
        match &self.outcome {
            Outcome::Ready(published) => published
                .textures
                .iter()
                .map(|texture| texture.rgba.len() as u64)
                .sum(),
            _ => 0,
        }
    }
}

/// The host's blend class for `blend`.
fn blend(blend: WorldBlend) -> Blend {
    match blend {
        WorldBlend::Opaque => Blend::Opaque,
        WorldBlend::Cutout => Blend::Cutout,
        WorldBlend::Premultiplied => Blend::Premultiplied,
        WorldBlend::Additive => Blend::Additive,
    }
}

/// The host's depth test for `depth`.
fn depth(depth: WorldDepth) -> Depth {
    match depth {
        WorldDepth::Test => Depth::Test,
        WorldDepth::Always => Depth::Always,
    }
}

/// Validates the shader again and reads the declared textures' sizes from their headers,
/// decoding nothing; the helper's validation is not trusted.
fn measure(source: &str, textures: &[String], files: &Files) -> Result<Measured, String> {
    let material = abi::compose(source, textures.len())?;
    let mut texels = 0_u64;
    for path in textures {
        let (width, height) = dimensions(signed(path, files)?)
            .ok_or_else(|| format!("{path} is not a bounded PNG"))?;
        texels += u64::from(width) * u64::from(height) * 4;
    }
    Ok(Measured {
        material: Arc::new(material),
        texels,
    })
}

/// The signed bytes of the texture at `path`.
fn signed<'a>(path: &str, files: &'a Files) -> Result<&'a [u8], String> {
    files
        .textures
        .iter()
        .find(|(listed, _)| listed == path)
        .map(|(_, bytes)| bytes.as_slice())
        .ok_or_else(|| format!("{path} is not a signed texture"))
}

/// Decodes every texture of `paths`.
fn decode_all(paths: &[String], files: &Files) -> Result<Vec<Texture>, String> {
    paths
        .iter()
        .map(|path| {
            decode(signed(path, files)?).ok_or_else(|| format!("{path} is not a bounded PNG"))
        })
        .collect()
}

/// A PNG reader bounded to [`MAX_TEXTURE_EXTENT`] texels a side.
fn reader(bytes: &[u8]) -> image::ImageReader<Cursor<&[u8]>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_EXTENT);
    limits.max_image_height = Some(MAX_TEXTURE_EXTENT);
    limits.max_alloc = Some(u64::from(MAX_TEXTURE_EXTENT).pow(2) * 8);
    reader.limits(limits);
    reader
}

/// A bounded PNG's size from its header alone.
fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height) = reader(bytes).into_dimensions().ok()?;
    (width <= MAX_TEXTURE_EXTENT && height <= MAX_TEXTURE_EXTENT).then_some((width, height))
}

/// Decodes a PNG of at most [`MAX_TEXTURE_EXTENT`] texels a side to RGBA8.
fn decode(bytes: &[u8]) -> Option<Texture> {
    let image = reader(bytes).decode().ok()?.to_rgba8();
    Some(Texture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    })
}
