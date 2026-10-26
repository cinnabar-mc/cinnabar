//! The presenter's compute declarations: each program validated again and its cells and
//! pipeline charged to the bundle's GPU share, kept while only its parameters change, and
//! published only while the bundle may run extra passes.

use super::admission::{Charge, Executor, Orphan, Pending};
use super::{BundleWorld, PIPELINE_CHARGE_BYTES, Pipelines, WorldMaterialPresenter, params};
use mod_render::world_material::{
    compute,
    publication::{Blend, Depth, PublishedCompute},
};
use server_experience::{manifest::Permission, world_material::WorldComputeSpec};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// A compute declaration's last outcome.
pub(super) struct CachedCompute {
    spec: WorldComputeSpec,
    attempt: u64,
    outcome: Outcome,
    /// The compute this declaration replaced, published in its place while the new program
    /// validates, so materials reading the cells keep reading them.
    previous: Option<PublishedCompute>,
}

/// What validating a compute declaration has produced so far.
enum Outcome {
    /// Not validated yet; its bundle runs one admission job at a time.
    Unvalidated,
    /// Validating off the frame.
    Validating(Pending<Result<compute::WorldCompute, String>>),
    Ready(PublishedCompute),
    /// Refused for this reason until its program changes.
    Refused(String),
}

impl CachedCompute {
    /// Takes a finished validation's result.
    fn settle(&mut self) {
        let Outcome::Validating(job) = &mut self.outcome else {
            return;
        };
        self.outcome = match job.take() {
            Some(Ok(program)) => Outcome::Ready(PublishedCompute {
                key: self.attempt,
                compute: Arc::new(program),
                cells: self.spec.cells,
                params: params(&self.spec.params),
            }),
            Some(Err(reason)) => Outcome::Refused(reason),
            None => return,
        };
        self.previous = None;
    }

    /// Whether its validation job is running.
    pub(super) fn running(&self) -> bool {
        matches!(self.outcome, Outcome::Validating(_))
    }

    /// Whether its running validation has returned a result not taken yet.
    pub(super) fn finished(&self) -> bool {
        matches!(&self.outcome, Outcome::Validating(job) if job.ready())
    }

    /// Starts validating its program on `executor`, so the frame never waits for it.
    fn validate(&mut self, executor: &Executor, done: &Arc<AtomicU64>) {
        let source = self.spec.source.clone();
        self.outcome = Outcome::Validating(executor.spawn(
            move |cancel| {
                if cancel.load(Ordering::Acquire) {
                    return Err("superseded".into());
                }
                compute::compose(&source)
            },
            done,
        ));
        self.settle();
    }
}

/// The identity of a compute's pipeline, marked apart from every material's set.
fn pipelines(published: &PublishedCompute) -> Pipelines {
    (
        published.compute.program(),
        Blend::Opaque,
        Depth::Test,
        1 << 7,
    )
}

/// The admission attempt of the compute declaration cached under `key`, if one is.
pub(super) fn attempt(
    computes: &BTreeMap<(String, u32), CachedCompute>,
    key: &(String, u32),
) -> Option<u64> {
    computes.get(key).map(|cached| cached.attempt)
}

impl WorldMaterialPresenter {
    /// Drops the compute declarations no bundle declares any more, a validation still running
    /// kept as its bundle's orphan until it returns.
    pub(super) fn drop_computes(&mut self, live: &BTreeSet<(String, u32)>) {
        let removed: Vec<(String, u32)> = self
            .computes
            .keys()
            .filter(|key| !live.contains(*key))
            .cloned()
            .collect();
        for key in removed {
            if let Some(CachedCompute {
                outcome: Outcome::Validating(job),
                ..
            }) = self.computes.remove(&key)
            {
                self.orphans.push((
                    (self.run, key.0),
                    Orphan {
                        alive: job.abandon(),
                        charge: Charge { gpu: 0, memory: 0 },
                    },
                ));
            }
        }
    }

    /// Publishes `bundle`'s compute declarations into `computes` while it may run extra
    /// passes, keeping them cached while that permission is withdrawn, and charges each its
    /// cells and, unless the bundle compiled it already this session, its pipeline against
    /// `budget` (GPU bytes left, host memory left); `pipelines` holds the bundle's compiled
    /// sets and the sets claimed across bundles this rebuild. A new program validates off the
    /// frame as the bundle's one admission job, which `busy` says is taken; until it is valid
    /// the compute it replaced stays published, or none is. Returns where each published one
    /// landed, by declaration id; `live` collects the declarations seen.
    pub(super) fn publish_computes(
        &mut self,
        bundle: &BundleWorld<'_>,
        (computes, live): (&mut Vec<PublishedCompute>, &mut BTreeSet<(String, u32)>),
        (gpu, memory): (&mut u64, &mut u64),
        (compiled, claimed): (&mut HashSet<Pipelines>, &mut HashSet<Pipelines>),
        busy: &mut bool,
    ) -> BTreeMap<u32, u16> {
        let mut slots = BTreeMap::new();
        if !bundle
            .allowed()
            .contains(&Permission::RenderWorldMaterialPasses)
        {
            // Withdrawn, the declarations stay cached, so restoring the permission brings back
            // the same computes, whose pipelines stay charged for the session anyway.
            live.extend(
                bundle
                    .world
                    .computes()
                    .keys()
                    .map(|id| (bundle.bundle.to_owned(), *id)),
            );
            return slots;
        }
        for (&id, spec) in bundle.world.computes() {
            let key = (bundle.bundle.to_owned(), id);
            live.insert(key.clone());
            let mut cached = match self.computes.remove(&key) {
                Some(mut cached) if cached.spec.same_program(spec) => {
                    if let Outcome::Ready(published) = &mut cached.outcome {
                        published.params = params(&spec.params);
                    }
                    cached.spec = spec.clone();
                    cached
                }
                previous => {
                    let mut replaced = None;
                    if let Some(previous) = previous {
                        replaced = previous.previous;
                        match previous.outcome {
                            Outcome::Ready(published) => replaced = Some(published),
                            // A superseded validation keeps the bundle's job until it returns.
                            Outcome::Validating(job) => self.orphans.push((
                                (self.run, bundle.bundle.to_owned()),
                                Orphan {
                                    alive: job.abandon(),
                                    charge: Charge { gpu: 0, memory: 0 },
                                },
                            )),
                            Outcome::Unvalidated | Outcome::Refused(_) => {}
                        }
                    }
                    self.next_key += 1;
                    CachedCompute {
                        spec: spec.clone(),
                        attempt: self.next_key,
                        outcome: Outcome::Unvalidated,
                        previous: replaced,
                    }
                }
            };
            cached.settle();
            if !*busy && matches!(cached.outcome, Outcome::Unvalidated) {
                cached.validate(&self.executor, &self.done);
            }
            // A validation still running, started now or by an earlier rebuild, is the
            // bundle's one job.
            *busy |= cached.running();
            let attempt = cached.attempt;
            let outcome = match &cached.outcome {
                Outcome::Ready(published) => Ok(Some(published.clone())),
                Outcome::Unvalidated | Outcome::Validating(_) => Ok(cached.previous.clone()),
                Outcome::Refused(reason) => Err(reason.clone()),
            };
            self.computes.insert(key, cached);
            let published = match outcome {
                Ok(Some(published)) => published,
                Ok(None) => continue,
                Err(reason) => {
                    self.report(bundle.bundle, id, attempt, &format!("compute: {reason}"));
                    continue;
                }
            };
            let identity = pipelines(&published);
            let pipeline = if compiled.contains(&identity) {
                0
            } else {
                PIPELINE_CHARGE_BYTES
            };
            // Two generations of four 32-bit numbers per cell of what is published.
            let charge = (u64::from(published.cells) * 2 * 16).saturating_add(pipeline);
            if charge > *gpu {
                self.report(
                    bundle.bundle,
                    id,
                    attempt,
                    "compute exceeds the bundle's GPU limit",
                );
                continue;
            }
            let host_bytes = (spec.source.len() + spec.params.len() * 4) as u64;
            if host_bytes > *memory {
                self.report(
                    bundle.bundle,
                    id,
                    attempt,
                    "compute exceeds the bundle's memory limit",
                );
                continue;
            }
            let Ok(slot) = u16::try_from(computes.len()) else {
                self.report(
                    bundle.bundle,
                    id,
                    attempt,
                    "compute past the session's compute slots",
                );
                continue;
            };
            if let Some(reason) = self.claim(identity, claimed) {
                self.report(bundle.bundle, id, attempt, reason);
                continue;
            }
            *gpu -= charge;
            *memory -= host_bytes;
            claimed.remove(&identity);
            compiled.insert(identity);
            self.retained
                .entry(identity)
                .or_insert_with(|| self.server.clone());
            computes.push(published);
            slots.insert(id, slot);
        }
        slots
    }

    /// Reports, once each, the published computes whose native pipelines `failed` to compile;
    /// materials keep reading their zeroed cells.
    pub(super) fn note_compute_failures(&mut self, failed: impl Fn(u64) -> bool) {
        let failures: Vec<(String, u32, u64)> = self
            .computes
            .iter()
            .filter_map(|((bundle, id), cached)| {
                let Outcome::Ready(published) = &cached.outcome else {
                    return None;
                };
                failed(published.key).then(|| (bundle.clone(), *id, cached.attempt))
            })
            .collect();
        for (bundle, id, attempt) in failures {
            self.report(
                &bundle,
                id,
                attempt,
                "compute failed to compile on this GPU",
            );
        }
    }
}
