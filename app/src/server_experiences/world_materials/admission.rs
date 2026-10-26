//! Admitting one declaration off the frame: validating its shader and reading its textures'
//! sizes, then, once its charge fits, decoding its textures. Each step runs on the async
//! compute pool and the presenter picks up its result on a later frame.

use super::{MAX_TEXTURE_EXTENT, PIPELINE_CHARGE_BYTES};
use mod_render::world_material::{
    self as abi,
    publication::{Blend, Depth, Published, Texture},
};
use server_experience::{
    screen::Files,
    world_material::{WorldBlend, WorldDepth, WorldEffects, WorldMaterialSpec},
};
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
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

/// A running job: the slot its result arrives in and the flags it shares with the job.
/// Dropping a pool job's handle cancels it only if it has not started; a started job runs
/// until it returns, which its alive flag reports.
pub(super) struct Pending<T> {
    result: Arc<Mutex<Option<T>>>,
    /// Keeps the pool task, which dropping cancels if it has not started.
    _task: Option<bevy::tasks::Task<()>>,
    /// Set until the job returns, or is dropped without running.
    alive: Arc<AtomicBool>,
    /// Tells the job it is superseded, so it stops at its next step.
    cancel: Arc<AtomicBool>,
}

impl<T> Pending<T> {
    /// Takes the result if the job finished.
    fn take(&mut self) -> Option<T> {
        self.result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Whether the job's result arrived and awaits [`Self::take`].
    fn ready(&self) -> bool {
        self.result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Tells the job to stop and returns the flag that clears once it really has.
    fn abandon(self) -> Arc<AtomicBool> {
        self.cancel.store(true, Ordering::Release);
        self.alive
    }
}

/// Clears a job's alive flag when the job returns or is dropped unrun.
struct Alive(Arc<AtomicBool>);

impl Drop for Alive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// A superseded job still running, whose bundle starts nothing new and whose charge stays
/// reserved until it returns.
pub(super) struct Orphan {
    pub alive: Arc<AtomicBool>,
    pub charge: Charge,
}

impl Orphan {
    /// Whether the job still runs.
    pub(super) fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

impl Executor {
    /// Starts `job`, handing it the cancel flag it checks between steps. A finished job
    /// publishes its result, then clears its alive flag, then counts itself in `done`, so
    /// whoever sees either signal finds the result already there.
    fn spawn<T: Send + 'static>(
        &self,
        job: impl FnOnce(&AtomicBool) -> T + Send + 'static,
        done: &Arc<AtomicU64>,
    ) -> Pending<T> {
        let alive = Arc::new(AtomicBool::new(true));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let (done, guard, flag, slot) = (
            Arc::clone(done),
            Alive(Arc::clone(&alive)),
            Arc::clone(&cancel),
            Arc::clone(&result),
        );
        let job = move || {
            let value = job(&flag);
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
            drop(guard);
            done.fetch_add(1, Ordering::Release);
        };
        let task = match self {
            Executor::Pool => Some(
                bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default)
                    .spawn(async move { job() }),
            ),
            #[cfg(test)]
            Executor::Inline => {
                job();
                None
            }
            #[cfg(test)]
            Executor::Held(held) => {
                held.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Box::new(job));
                None
            }
        };
        Pending {
            result,
            _task: task,
            alive,
            cancel,
        }
    }
}

/// What admitting a declaration has produced so far.
pub(super) enum Outcome {
    /// Not measured yet; its bundle runs one admission job at a time.
    Unmeasured,
    /// Validating its shader and reading its textures' sizes.
    Measuring(Pending<Result<Measured, String>>),
    /// A valid shader that is not drawn. It holds no decoded texels.
    Waiting(Measured),
    /// Decoding its textures, its charge reserved.
    Decoding(Measured, Pending<Result<Arc<[Texture]>, String>>),
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

/// What a material's pipelines are compiled for: its program, blend, depth and the extra
/// passes it draws ([`passes`]). Bevy keeps one set per distinct identity.
pub(super) type Pipelines = (u128, Blend, Depth, u8);

/// The extra passes `effects` draw, one bit each: outline, bloom and particles.
pub(super) fn passes(effects: &WorldEffects) -> u8 {
    u8::from(effects.outline.is_some())
        | u8::from(effects.bloom.is_some()) << 1
        | u8::from(effects.particles.is_some()) << 2
}

/// GPU bytes a set of pipelines holds: one pipeline charge for its shade pass and one per
/// extra pass.
pub(super) fn set_bytes(identity: &Pipelines) -> u64 {
    PIPELINE_CHARGE_BYTES * (1 + u64::from((identity.3 & 0b111).count_ones()))
}

/// How fitting a material against its bundle's shares went.
pub(super) enum Fit {
    Drawable(Published, Charge),
    /// Its charge fits and its textures await decoding.
    Decode(Charge),
    /// Being decoded, with this charge reserved, or not measured yet; not a failure.
    Pending(Charge),
    Refused(String),
}

/// A declaration's last outcome, kept while its program is unchanged.
pub(super) struct Cached {
    /// The declaration with only the effects its bundle may draw now.
    pub spec: WorldMaterialSpec,
    /// The effects the declaration asked for, before any was dropped, which the cache keys
    /// on too: an effect never granted refuses the declaration.
    pub declared: WorldEffects,
    /// Its superseded jobs still running, which the presenter takes with [`Self::orphans`].
    abandoned: Vec<Orphan>,
    /// Numbers this admission attempt, so a failure is reported once per attempt; it is also
    /// the material's key, and a newer attempt's job replaces an older one's.
    pub attempt: u64,
    pub outcome: Outcome,
}

impl Cached {
    /// A new admission attempt of `spec`, which `declared` effects permit to draw, not
    /// measured yet.
    pub(super) fn new(spec: &WorldMaterialSpec, declared: WorldEffects, attempt: u64) -> Self {
        Self {
            spec: spec.clone(),
            declared,
            abandoned: Vec::new(),
            attempt,
            outcome: Outcome::Unmeasured,
        }
    }

    /// Whether this attempt's running job has returned a result not taken yet.
    pub(super) fn finished(&self) -> bool {
        match &self.outcome {
            Outcome::Measuring(job) => job.ready(),
            Outcome::Decoding(_, job) => job.ready(),
            _ => false,
        }
    }

    /// An admission attempt of `spec`, declared with `declared` effects, refused for `reason`
    /// before any work.
    pub(super) fn refused(
        spec: &WorldMaterialSpec,
        declared: WorldEffects,
        attempt: u64,
        reason: String,
    ) -> Self {
        Self {
            outcome: Outcome::Refused(reason),
            ..Self::new(spec, declared, attempt)
        }
    }

    /// Whether a job of this attempt is running.
    pub(super) fn running(&self) -> bool {
        matches!(self.outcome, Outcome::Measuring(_) | Outcome::Decoding(..))
    }

    /// Whether its shader and texture sizes are known, or it was refused.
    pub(super) fn measured(&self) -> bool {
        !matches!(self.outcome, Outcome::Unmeasured | Outcome::Measuring(_))
    }

    /// Whether it still needs measuring.
    pub(super) fn unmeasured(&self) -> bool {
        matches!(self.outcome, Outcome::Unmeasured)
    }

    /// Starts validating its shader and reading its textures' headers on `executor`, so the
    /// frame never waits for either.
    pub(super) fn measure(
        &mut self,
        files: &Arc<Files>,
        executor: &Executor,
        done: &Arc<AtomicU64>,
    ) {
        let (source, textures, files) = (
            self.spec.source.clone(),
            self.spec.textures.clone(),
            Arc::clone(files),
        );
        let features = mod_host::server::world_material::features(&self.spec.effects);
        self.outcome = Outcome::Measuring(executor.spawn(
            move |cancel| measure(&source, (&textures, features), &files, cancel),
            done,
        ));
        self.settle();
    }

    /// Starts decoding its textures once [`Self::fit`] said they fit.
    pub(super) fn decode(
        &mut self,
        files: &Arc<Files>,
        executor: &Executor,
        done: &Arc<AtomicU64>,
    ) {
        let Outcome::Waiting(measured) = &self.outcome else {
            return;
        };
        let (textures, files) = (self.spec.textures.clone(), Arc::clone(files));
        let job = executor.spawn(
            move |cancel| decode_all(&textures, &files, cancel).map(Arc::from),
            done,
        );
        self.outcome = Outcome::Decoding(measured.clone(), job);
        self.settle();
    }

    /// Abandons its running job, which stops at its next step: the attempt goes back to the
    /// step that job was taking, and the job stays an orphan, its decode charged, until it
    /// returns.
    pub(super) fn cancel(&mut self) {
        let (alive, charge, outcome) =
            match std::mem::replace(&mut self.outcome, Outcome::Unmeasured) {
                Outcome::Measuring(job) => (
                    job.abandon(),
                    Charge { gpu: 0, memory: 0 },
                    Outcome::Unmeasured,
                ),
                Outcome::Decoding(measured, job) => (
                    job.abandon(),
                    Charge {
                        gpu: measured.texels,
                        memory: measured.texels,
                    },
                    Outcome::Waiting(measured),
                ),
                other => {
                    self.outcome = other;
                    return;
                }
            };
        self.outcome = outcome;
        self.abandoned.push(Orphan { alive, charge });
    }

    /// Takes its abandoned jobs that may still run.
    pub(super) fn orphans(&mut self) -> Vec<Orphan> {
        std::mem::take(&mut self.abandoned)
    }

    /// The identity of the pipelines this material compiles, once measured.
    pub(super) fn pipelines(&mut self) -> Option<Pipelines> {
        self.settle();
        let material = match &self.outcome {
            Outcome::Waiting(measured) | Outcome::Decoding(measured, _) => &measured.material,
            Outcome::Ready(published) => &published.material,
            Outcome::Unmeasured | Outcome::Measuring(_) | Outcome::Refused(_) => return None,
        };
        Some((
            material.program(),
            blend(self.spec.blend),
            depth(self.spec.depth),
            passes(&self.spec.effects),
        ))
    }

    /// Charges the material against the `gpu` and `memory` bytes its bundle has left, where
    /// `pipeline` is what its pipelines and bind group cost. A material that no longer fits
    /// gives up its decoded textures and any decode in flight, so the host never holds texels
    /// its shares do not cover.
    pub(super) fn fit(&mut self, gpu: u64, memory: u64, pipeline: u64) -> Fit {
        self.settle();
        let measured = match &self.outcome {
            Outcome::Unmeasured | Outcome::Measuring(_) => {
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
            self.cancel();
            self.outcome = Outcome::Waiting(measured);
            return Fit::Refused(reason.into());
        }
        match &self.outcome {
            Outcome::Ready(published) => Fit::Drawable(published.clone(), charge),
            Outcome::Waiting(_) => Fit::Decode(charge),
            _ => Fit::Pending(charge),
        }
    }

    /// Takes any finished job's result.
    pub(super) fn settle(&mut self) {
        let next = match &mut self.outcome {
            Outcome::Measuring(job) => match job.take() {
                Some(Ok(measured)) => Outcome::Waiting(measured),
                Some(Err(reason)) => Outcome::Refused(reason),
                None => return,
            },
            Outcome::Decoding(measured, job) => match job.take() {
                Some(Ok(textures)) => Outcome::Ready(Published {
                    key: self.attempt,
                    material: Arc::clone(&measured.material),
                    textures,
                    depth: depth(self.spec.depth),
                    blend: blend(self.spec.blend),
                    // The presenter resolves them against each rebuild's computes.
                    effects: Default::default(),
                }),
                Some(Err(reason)) => Outcome::Refused(reason),
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

/// Validates the shader, with the functions its effects call, again and reads the declared
/// textures' sizes from their headers, decoding nothing; the helper's validation is not
/// trusted.
fn measure(
    source: &str,
    (textures, features): (&[String], abi::Features),
    files: &Files,
    cancel: &AtomicBool,
) -> Result<Measured, String> {
    if cancel.load(Ordering::Acquire) {
        return Err(SUPERSEDED.into());
    }
    let material = abi::compose_with(source, textures.len(), features)?;
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

/// Why a superseded job stopped early.
const SUPERSEDED: &str = "superseded";

/// Decodes every texture of `paths`, stopping before the next one once `cancel` is set.
pub(super) fn decode_all(
    paths: &[String],
    files: &Files,
    cancel: &AtomicBool,
) -> Result<Vec<Texture>, String> {
    paths
        .iter()
        .map(|path| {
            if cancel.load(Ordering::Acquire) {
                return Err(SUPERSEDED.into());
            }
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
