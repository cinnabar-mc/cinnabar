//! The presenter's compute declarations: each program validated again and its cells and
//! pipeline charged to the bundle's GPU share, kept while only its parameters change, and
//! published only while the bundle may run extra passes.

use super::{BundleWorld, PIPELINE_CHARGE_BYTES, Pipelines, WorldMaterialPresenter, params};
use mod_render::world_material::{
    compute,
    publication::{Blend, Depth, PublishedCompute},
};
use server_experience::{manifest::Permission, world_material::WorldComputeSpec};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

/// A compute declaration's last outcome.
pub(super) struct CachedCompute {
    spec: WorldComputeSpec,
    attempt: u64,
    outcome: Result<PublishedCompute, String>,
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
    /// Publishes `bundle`'s compute declarations into `computes` while it may run extra
    /// passes, keeping them cached while that permission is withdrawn, charging each its cells and, unless the bundle compiled it already this session,
    /// its pipeline against `budget` (GPU bytes left, host memory left); `pipelines` holds the
    /// bundle's compiled sets and the sets claimed across bundles this rebuild. Returns where
    /// each published one landed, by declaration id; `live` collects the declarations seen.
    pub(super) fn publish_computes(
        &mut self,
        bundle: &BundleWorld<'_>,
        (computes, live): (&mut Vec<PublishedCompute>, &mut BTreeSet<(String, u32)>),
        (gpu, memory): (&mut u64, &mut u64),
        (compiled, claimed): (&mut HashSet<Pipelines>, &mut HashSet<Pipelines>),
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
            let (attempt, outcome) = match self.computes.get_mut(&key) {
                Some(cached) if cached.spec.same_program(spec) => {
                    if let Ok(published) = &mut cached.outcome {
                        published.params = params(&spec.params);
                    }
                    cached.spec = spec.clone();
                    (cached.attempt, cached.outcome.clone())
                }
                _ => {
                    self.next_key += 1;
                    let outcome = compute::compose(&spec.source).map(|program| PublishedCompute {
                        key: self.next_key,
                        compute: Arc::new(program),
                        cells: spec.cells,
                        params: params(&spec.params),
                    });
                    self.computes.insert(
                        key.clone(),
                        CachedCompute {
                            spec: spec.clone(),
                            attempt: self.next_key,
                            outcome: outcome.clone(),
                        },
                    );
                    (self.next_key, outcome)
                }
            };
            let published = match outcome {
                Ok(published) => published,
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
            let charge = spec.cell_bytes().saturating_add(pipeline);
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
            if let Some(reason) = self.claim(identity, claimed) {
                self.report(bundle.bundle, id, attempt, reason);
                continue;
            }
            let Ok(slot) = u16::try_from(computes.len()) else {
                continue;
            };
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
                let published = cached.outcome.as_ref().ok()?;
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
