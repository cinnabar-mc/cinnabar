//! Ordered frame-budget reservations over disjoint actor samples.
use super::*;

mod workers;

/// Minimum costly samples before dispatching to the shared pool.
const MIN_PARALLEL_SAMPLES: usize = 8;

struct Sample<'a> {
    id: u64,
    layers: Option<ActorRenderLayers<'a>>,
    used: usize,
}

impl<'a> ActorRenderFrame<'a> {
    /// Samples the requested actors in order, sharing results with later frame consumers.
    /// Workers receive full actor allowances only when their whole run fits the frame budget;
    /// the remaining tail uses the serial allowance, including its exact exhaustion point.
    /// The caller runs unclaimed work and never waits for a helper that has not started.
    pub fn sample_ordered(&mut self, runtime_ids: &[u64]) {
        if runtime_ids.len() < MIN_PARALLEL_SAMPLES
            || self.remaining_ops / MAX_MOLANG_OPS_PER_ACTOR_FRAME < MIN_PARALLEL_SAMPLES
            || rayon::current_num_threads() == 1
            || runtime_ids
                .iter()
                .filter(|&&id| self.needs_worker(id))
                .take(MIN_PARALLEL_SAMPLES)
                .count()
                < MIN_PARALLEL_SAMPLES
        {
            for &id in runtime_ids {
                self.cached_layers(id);
            }
            return;
        }
        let mut samples = Vec::with_capacity(runtime_ids.len());
        for &id in runtime_ids {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.sampled_layers.entry(id)
            {
                entry.insert(None);
                samples.push(Sample {
                    id,
                    layers: None,
                    used: 0,
                });
            }
        }
        let mut next = 0;
        while next < samples.len() {
            let mut end = next;
            let mut reserved = 0;
            while let Some(sample) = samples.get(end) {
                let allowance = self.sample_budget(sample.id);
                if allowance > self.remaining_ops - reserved {
                    break;
                }
                reserved += allowance;
                end += 1;
            }
            let run = &mut samples[next..end];
            let costly = run
                .iter()
                .filter(|sample| self.needs_worker(sample.id))
                .count();
            if costly >= MIN_PARALLEL_SAMPLES && rayon::current_num_threads() > 1 {
                workers::sample(run, self);
                for sample in run {
                    self.remaining_ops -= sample.used;
                }
                next = end;
            } else {
                let end = end.max(next + 1);
                for sample in &mut samples[next..end] {
                    sample.layers = Self::sample(
                        self.store,
                        self.actors,
                        (self.camera_rotation, self.camera_position, self.alpha),
                        sample.id,
                        &mut self.remaining_ops,
                        true,
                    );
                }
                next = end;
            }
        }
        for sample in samples {
            self.sampled_layers.insert(sample.id, sample.layers);
        }
    }

    /// Reserves frame replay work for the requested actor's authored scripts.
    fn sample_budget(&self, id: u64) -> usize {
        let state = self
            .store
            .runtime_to_lifetime
            .get(&id)
            .and_then(|lifetime| self.store.rigs.get(lifetime));
        let allowance = state.and_then(|state| {
            let assets = if state.pack {
                self.store.pack.as_ref().map(|pack| &*pack.assets)
            } else {
                self.store.assets.as_deref()
            }?;
            Some(molang_tick_budget(assets, state.rig_binding))
        });
        FRAME_PASSES_PER_ACTOR * allowance.unwrap_or(MAX_MOLANG_OPS_PER_ACTOR_TICK)
    }

    /// Keeps rigs that only return completed tick layers on the caller's inline path.
    fn needs_worker(&self, id: u64) -> bool {
        self.actors.animated_actor(id).is_some_and(|actor| {
            self.store
                .runtime_to_lifetime
                .get(&id)
                .filter(|lifetime| lifetime.spawn_revision == actor.spawn_revision())
                .and_then(|lifetime| self.store.rigs.get(lifetime))
                .filter(|state| !state.reset_pending)
                .is_some_and(|state| {
                    state.render_frame.as_ref().is_some_and(|frame| {
                        state.samples_render_frames
                            || state.samples_swing_poses
                            || frame.needs_swell_sampling(actor)
                            || frame.needs_rotation_sampling()
                            || frame.needs_walk_sampling()
                    })
                })
        })
    }
}

#[cfg(test)]
mod tests;
