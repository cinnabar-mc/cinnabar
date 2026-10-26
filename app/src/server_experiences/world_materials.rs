//! Turns the client parts' retained world material declarations into the set the renderer
//! draws: each shader validated again and each declared texture decoded off the frame, every
//! material charged to its bundle's GPU and memory shares, and every binding resolved to a
//! slot. A parameter-only change patches the published parameters without a rebuild.

mod admission;
mod slots;

use admission::{Cached, Executor, Fit, Pipelines};
use mod_render::world_material::publication::{
    self, Published, WorldMaterialParams, WorldMaterialSet,
};
use server_experience::{
    screen::Files,
    world_material::{WorldMaterials, WorldTarget},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Provisional: the widest or tallest declared texture, in texels.
pub(super) const MAX_TEXTURE_EXTENT: u32 = 1024;
/// Provisional: GPU bytes charged for each distinct set of pipelines a bundle compiles.
pub(super) const PIPELINE_CHARGE_BYTES: u64 = 256 * 1024;
/// Provisional: GPU bytes charged for each drawn material's uniform and bind group, on top of
/// its textures.
pub(super) const BIND_GROUP_CHARGE_BYTES: u64 = 4 * 1024;
/// Provisional: GPU bytes the process's retained pipelines may charge across every session,
/// one bundle's largest signed GPU share. Bevy cannot drop a compiled pipeline, so once it is
/// full a program new to the process is refused while those already compiled keep drawing.
pub(super) const PROCESS_PIPELINE_BYTES: u64 = server_experience::policy::MAX_GPU_BYTES;
/// Failure descriptions kept for the session's diagnostics, newest last.
pub(super) const MAX_DIAGNOSTICS: usize = 16;

/// One bundle's declarations as the presenter reads them.
pub(super) struct BundleWorld<'a> {
    pub bundle: &'a str,
    pub world: &'a WorldMaterials,
    /// The bundle's share of the signed GPU limit.
    pub gpu_bytes: u64,
    /// What the bundle's share of the signed memory limit leaves once its retained
    /// declarations are charged; the host's composed shaders and decoded textures draw from it,
    /// so the client's copies share one ledger. The guest's own Wasm memory lives in the helper
    /// process, which bounds it by the same signed limit separately.
    pub memory_bytes: u64,
    /// The bundle's signed files, whose PNGs the declarations name.
    pub files: &'a Arc<Files>,
}

pub(super) struct WorldMaterialPresenter {
    cache: BTreeMap<(String, u32), Cached>,
    next_key: u64,
    revision: u64,
    set: Arc<WorldMaterialSet>,
    params: Arc<WorldMaterialParams>,
    /// Failures already reported, so each is logged once.
    reported: BTreeSet<(String, u32, u64, String)>,
    diagnostics: VecDeque<String>,
    /// The pipelines each bundle drew with this session. Bevy keeps every compiled pipeline,
    /// so each distinct set stays charged to the bundle's GPU share until the session ends.
    compiled: BTreeMap<String, HashSet<Pipelines>>,
    /// Every set of pipelines the process compiled, across sessions.
    retained: HashSet<Pipelines>,
    slots: slots::Slots,
    /// What the render world last observed of the block slots meshes carry.
    carried: slots::Carried,
    /// How many empty block slots another bundle could take at the last look.
    reclaimable: usize,
    /// Whether a drawable material found no slot, so a freed slot should rebuild.
    starved: bool,
    /// The runtime and world contribution revision the set was last built from.
    built: Option<(u64, u64)>,
    /// The runtime the session belongs to; zero while none runs.
    run: u64,
    executor: Executor,
    /// Counts finished admission jobs; a change publishes their results.
    done: Arc<AtomicU64>,
    /// The count of finished jobs the set was last built after.
    seen: u64,
}

impl Default for WorldMaterialPresenter {
    fn default() -> Self {
        Self::with_executor(Executor::default())
    }
}

impl WorldMaterialPresenter {
    /// A presenter whose admission work runs on `executor`; until told otherwise every block
    /// slot counts as carried.
    fn with_executor(executor: Executor) -> Self {
        Self {
            cache: BTreeMap::new(),
            next_key: 0,
            revision: 0,
            set: Arc::default(),
            params: Arc::default(),
            reported: BTreeSet::new(),
            diagnostics: VecDeque::new(),
            compiled: BTreeMap::new(),
            retained: HashSet::new(),
            slots: slots::Slots::default(),
            carried: slots::Carried::default(),
            reclaimable: 0,
            starved: false,
            built: None,
            run: 0,
            executor,
            done: Arc::default(),
            seen: 0,
        }
    }

    /// A presenter that admits on the calling thread, so each rebuild settles at once.
    #[cfg(test)]
    pub(super) fn inline() -> Self {
        Self::with_executor(Executor::Inline)
    }

    /// The set to draw.
    pub(super) fn set(&self) -> &Arc<WorldMaterialSet> {
        &self.set
    }

    /// Each drawn material's parameters.
    pub(super) fn params(&self) -> &Arc<WorldMaterialParams> {
        &self.params
    }

    /// The runtime the session belongs to; zero while none runs.
    pub(super) fn run(&self) -> u64 {
        self.run
    }

    /// The most recent failures, oldest first, at most [`MAX_DIAGNOSTICS`].
    pub(super) fn diagnostics(&self) -> impl Iterator<Item = &str> {
        self.diagnostics.iter().map(String::as_str)
    }

    /// Follows `live`'s declarations: another runtime, or none, ends the previous session
    /// first; a structural change, finished admission work or a freed slot a material waits
    /// for rebuilds the set; parameter-only changes patch the published parameters. `carried`
    /// is the render world's latest (observation count, carried block slots). Returns whether
    /// the set or the parameters changed.
    pub(super) fn follow<H: super::worker::Worker>(
        &mut self,
        live: Option<&mut super::live::Live<H>>,
        (count, mask): (u64, u64),
    ) -> bool {
        let carried = slots::Carried { count, mask };
        let built = live.as_deref().map(super::live::Live::world_contribution);
        let run = built.map_or(0, |(run, _)| run);
        let mut changed = false;
        if run != self.run {
            changed = self.end_session();
            self.run = run;
        }
        // A slot that became reclaimable may go to a material that found none.
        let reclaimable = self.slots.reclaimable(carried);
        let freed = self.starved && reclaimable > self.reclaimable;
        self.reclaimable = reclaimable;
        self.carried = carried;
        let finished = self.done.load(Ordering::Acquire);
        let settled = finished != self.seen;
        self.seen = finished;
        let Some(live) = live else {
            return changed;
        };
        let patches = live.take_world_params();
        if built != self.built || settled || freed {
            self.built = built;
            changed |= self.rebuild(live.world_bundles());
        } else {
            for (bundle, id, values) in patches {
                changed |= self.patch(&bundle, id, &values);
            }
        }
        changed
    }

    /// Forgets the session's declarations, charges and diagnostics and empties its slots,
    /// publishing an empty set when the drawn one was not; returns whether it published. Each
    /// slot stays reserved for its owner until no mesh carries its tag, and keys and revisions
    /// keep counting, so a renderer never mistakes a new session's material for an old one.
    pub(super) fn end_session(&mut self) -> bool {
        self.cache.clear();
        self.reported.clear();
        self.diagnostics.clear();
        self.compiled.clear();
        self.slots.empty(self.carried.count);
        self.starved = false;
        self.built = None;
        let mut changed = false;
        if *self.set != WorldMaterialSet::empty(self.set.revision) {
            self.revision += 1;
            self.set = Arc::new(WorldMaterialSet::empty(self.revision));
            changed = true;
        }
        if !self.params.values.is_empty() {
            self.params = Arc::new(WorldMaterialParams {
                revision: self.params.revision + 1,
                values: BTreeMap::new(),
            });
            changed = true;
        }
        changed
    }

    /// Writes material `id` of `bundle`'s new parameters, publishing them when it is drawn;
    /// returns whether the published parameters changed.
    fn patch(&mut self, bundle: &str, id: u32, values: &[f32]) -> bool {
        let Some(cached) = self.cache.get_mut(&(bundle.to_owned(), id)) else {
            return false;
        };
        values.clone_into(&mut cached.spec.params);
        let next = params(values);
        match self.params.values.get(&cached.attempt) {
            Some(drawn) if *drawn != next => {
                let params = Arc::make_mut(&mut self.params);
                params.revision += 1;
                params.values.insert(cached.attempt, next);
                true
            }
            _ => false,
        }
    }

    /// Rebuilds the set from every bundle's declarations; returns whether the set or the
    /// parameters changed. An unchanged program keeps its compiled material, key and slot.
    pub(super) fn rebuild<'a>(
        &mut self,
        bundles: impl IntoIterator<Item = BundleWorld<'a>>,
    ) -> bool {
        let bundles: Vec<BundleWorld<'a>> = bundles.into_iter().collect();
        let mut drawn: BTreeMap<(String, u32), Published> = BTreeMap::new();
        let mut values = BTreeMap::new();
        let mut live = BTreeSet::new();
        let mut claimed = HashSet::new();
        for bundle in &bundles {
            self.admit(bundle, &mut drawn, &mut values, &mut live, &mut claimed);
        }
        self.cache.retain(|key, _| live.contains(key));
        // Attempts no longer cached cannot recur, so their reports go with them.
        let cache = &self.cache;
        self.reported.retain(|(bundle, id, attempt, _)| {
            cache
                .get(&(bundle.clone(), *id))
                .is_some_and(|cached| cached.attempt == *attempt)
        });
        let (materials, entities, blocks) = self.bind(&bundles, drawn);
        let mut changed = false;
        if materials != self.set.materials
            || entities != self.set.entities
            || blocks != self.set.blocks
        {
            self.revision += 1;
            self.set = Arc::new(WorldMaterialSet {
                revision: self.revision,
                materials,
                entities,
                blocks,
            });
            changed = true;
        }
        if values != self.params.values {
            self.params = Arc::new(WorldMaterialParams {
                revision: self.params.revision + 1,
                values,
            });
            changed = true;
        }
        changed
    }

    /// Admits `bundle`'s declarations against its shares, adding what draws to `drawn` and its
    /// parameters to `values` and every declaration to `live`. One admission job runs per
    /// bundle, and a newer attempt's job replaces an older one's; nothing new draws until all
    /// of the bundle's materials are measured; and a set of pipelines new to the process waits
    /// for room in the process's pipeline budget, which `claimed` reserves across bundles.
    fn admit(
        &mut self,
        bundle: &BundleWorld<'_>,
        drawn: &mut BTreeMap<(String, u32), Published>,
        values: &mut BTreeMap<u64, [f32; 16]>,
        live: &mut BTreeSet<(String, u32)>,
        claimed: &mut HashSet<Pipelines>,
    ) {
        let mut entries: Vec<(u32, Cached)> = Vec::new();
        for (&id, spec) in bundle.world.materials() {
            let key = (bundle.bundle.to_owned(), id);
            let cached = match self.cache.remove(&key) {
                // New parameters keep the compiled shader, its key and its charge.
                Some(mut cached) if cached.spec.same_program(spec) => {
                    if cached.spec.params != spec.params {
                        spec.params.clone_into(&mut cached.spec.params);
                    }
                    cached
                }
                _ => {
                    self.next_key += 1;
                    Cached::new(spec, self.next_key)
                }
            };
            live.insert(key);
            entries.push((id, cached));
        }
        for (_, cached) in &mut entries {
            cached.settle();
        }
        let mut job = entries.iter().position(|(_, cached)| cached.running());
        // The newest unmeasured attempt measures next; work that finishes at once lets the
        // next one start.
        while let Some(newest) = (0..entries.len())
            .filter(|&index| entries[index].1.unmeasured())
            .max_by_key(|&index| entries[index].1.attempt)
            && job.is_none_or(|running| entries[running].1.attempt < entries[newest].1.attempt)
        {
            if let Some(running) = job {
                entries[running].1.cancel();
            }
            entries[newest]
                .1
                .measure(bundle.files, &self.executor, &self.done);
            job = entries[newest].1.running().then_some(newest);
        }
        let measured = entries.iter().all(|(_, cached)| cached.measured());
        let mut compiled = self.compiled.remove(bundle.bundle).unwrap_or_default();
        // Every set of pipelines the bundle compiled this session stays charged.
        let mut gpu = bundle
            .gpu_bytes
            .saturating_sub(compiled.len() as u64 * PIPELINE_CHARGE_BYTES);
        let mut memory = bundle.memory_bytes;
        let mut reserved = HashSet::new();
        let mut newly = HashSet::new();
        for index in 0..entries.len() {
            let (id, attempt) = (entries[index].0, entries[index].1.attempt);
            let identity = entries[index].1.pipelines();
            let shared = identity.is_some_and(|identity| {
                compiled.contains(&identity) || reserved.contains(&identity)
            });
            // A program the bundle already compiled costs only its bind group.
            let pipeline = BIND_GROUP_CHARGE_BYTES + if shared { 0 } else { PIPELINE_CHARGE_BYTES };
            let mut fit = entries[index].1.fit(gpu, memory, pipeline);
            if let (Fit::Decode(_) | Fit::Drawable(..), Some(identity)) = (&fit, identity)
                && !self.retained.contains(&identity)
                && !claimed.contains(&identity)
            {
                let held = (self.retained.len() + claimed.len()) as u64 + 1;
                if held.saturating_mul(PIPELINE_CHARGE_BYTES) > PROCESS_PIPELINE_BYTES {
                    entries[index].1.cancel();
                    fit =
                        Fit::Refused("the client's world material pipeline memory is full".into());
                } else {
                    claimed.insert(identity);
                }
            }
            if let Fit::Decode(charge) = fit {
                // Nothing new draws before every material of the bundle is measured.
                if !measured {
                    continue;
                }
                let free = job
                    .is_none_or(|running| running == index || entries[running].1.attempt < attempt);
                if !free {
                    // Its charge stays reserved while the bundle's one job runs.
                    gpu -= charge.gpu;
                    memory -= charge.memory;
                    reserved.extend(identity);
                    continue;
                }
                if let Some(running) = job.filter(|running| *running != index) {
                    entries[running].1.cancel();
                }
                entries[index]
                    .1
                    .decode(bundle.files, &self.executor, &self.done);
                job = entries[index].1.running().then_some(index);
                fit = entries[index].1.fit(gpu, memory, pipeline);
            }
            match fit {
                Fit::Drawable(published, charge) => {
                    gpu -= charge.gpu;
                    memory -= charge.memory;
                    values.insert(published.key, params(&entries[index].1.spec.params));
                    drawn.insert((bundle.bundle.to_owned(), id), published);
                    reserved.extend(identity);
                    newly.extend(identity);
                    if let Some(identity) = identity {
                        claimed.remove(&identity);
                        self.retained.insert(identity);
                    }
                }
                Fit::Pending(charge) | Fit::Decode(charge) => {
                    gpu -= charge.gpu;
                    memory -= charge.memory;
                    if charge.gpu != 0 {
                        reserved.extend(identity);
                    }
                }
                Fit::Refused(reason) => self.report(bundle.bundle, id, attempt, &reason),
            }
        }
        compiled.extend(newly);
        self.compiled.insert(bundle.bundle.to_owned(), compiled);
        for (id, cached) in entries {
            self.cache.insert((bundle.bundle.to_owned(), id), cached);
        }
    }

    /// How many admission jobs run.
    #[cfg(test)]
    fn jobs(&self) -> usize {
        self.cache
            .values()
            .filter(|cached| cached.running())
            .count()
    }

    /// Gives every drawn material a slot and resolves every binding to its material's slot.
    #[allow(clippy::type_complexity, reason = "the set's three tables")]
    fn bind(
        &mut self,
        bundles: &[BundleWorld<'_>],
        mut drawn: BTreeMap<(String, u32), Published>,
    ) -> (
        Vec<Option<Published>>,
        BTreeMap<Box<str>, u16>,
        BTreeMap<Box<str>, u16>,
    ) {
        let mut block_bound = BTreeSet::new();
        for bundle in bundles {
            for binding in bundle.world.bindings().values() {
                if matches!(binding.target, WorldTarget::Block(_)) {
                    block_bound.insert((bundle.bundle, binding.material));
                }
            }
        }
        let requests: Vec<slots::Request<'_>> = drawn
            .keys()
            .map(|(bundle, id)| slots::Request {
                bundle,
                id: *id,
                blocks: block_bound.contains(&(bundle.as_str(), *id)),
            })
            .collect();
        let assigned = self.slots.assign(self.run, &requests, self.carried);
        drop(requests);
        self.starved = assigned.len() < drawn.len();
        let mut materials: Vec<Option<Published>> = vec![None; self.slots.len()];
        let mut by_bundle: BTreeMap<&str, BTreeMap<u32, u16>> = BTreeMap::new();
        for ((bundle, id), slot) in &assigned {
            if let Some(published) = drawn.remove(&(bundle.clone(), *id)) {
                materials[usize::from(*slot)] = Some(published);
            }
            by_bundle
                .entry(bundle.as_str())
                .or_default()
                .insert(*id, *slot);
        }
        let mut entities = BTreeMap::new();
        let mut blocks = BTreeMap::new();
        for bundle in bundles {
            let Some(slots) = by_bundle.get(bundle.bundle) else {
                continue;
            };
            for binding in bundle.world.bindings().values() {
                let Some(&slot) = slots.get(&binding.material) else {
                    continue;
                };
                let (map, identifier, reachable) = match &binding.target {
                    WorldTarget::Entity(identifier) => (
                        &mut entities,
                        identifier,
                        usize::from(slot) < publication::MAX_ENTITY_SLOTS,
                    ),
                    WorldTarget::Block(identifier) => (
                        &mut blocks,
                        identifier,
                        usize::from(slot) < publication::MAX_BLOCK_SLOTS,
                    ),
                };
                // Only the tag width limits which material a target can draw with.
                if !reachable {
                    let attempt = self
                        .cache
                        .get(&(bundle.bundle.to_owned(), binding.material))
                        .map_or(0, |cached| cached.attempt);
                    self.report(
                        bundle.bundle,
                        binding.material,
                        attempt,
                        "bound past the session's material slots for its target kind",
                    );
                    continue;
                }
                // Two bundles may sign one namespace; the first in bundle order keeps it.
                map.entry(identifier.as_str().into()).or_insert(slot);
            }
        }
        (materials, entities, blocks)
    }

    /// Reports, once each, the published materials whose native pipelines `failed` to compile;
    /// their draws keep the Bedrock material.
    pub(super) fn note_compile_failures(&mut self, failed: impl Fn(u64) -> bool) {
        let failures: Vec<(String, u32, u64)> = self
            .set
            .published()
            .filter(|published| failed(published.key))
            .filter_map(|published| {
                self.cache
                    .iter()
                    .find(|(_, cached)| cached.attempt == published.key)
                    .map(|((bundle, id), cached)| (bundle.clone(), *id, cached.attempt))
            })
            .collect();
        for (bundle, id, attempt) in failures {
            self.report(&bundle, id, attempt, "failed to compile on this GPU");
        }
    }

    /// Logs a failure once per declaration attempt and keeps it for the diagnostics.
    fn report(&mut self, bundle: &str, id: u32, attempt: u64, reason: &str) {
        if !self
            .reported
            .insert((bundle.to_owned(), id, attempt, reason.to_owned()))
        {
            return;
        }
        bevy::log::warn!(bundle, id, reason, "world material not drawn");
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics
            .push_back(format!("{bundle} world material {id}: {reason}"));
    }

    /// Bytes of decoded texels the presenter holds.
    #[cfg(test)]
    fn held_texels(&self) -> u64 {
        self.cache.values().map(Cached::held_texels).sum()
    }
}

/// A declaration's parameters as the sixteen the renderer uploads, missing ones zero.
fn params(declared: &[f32]) -> [f32; 16] {
    let mut params = [0.0; 16];
    for (slot, value) in params.iter_mut().zip(declared) {
        *slot = *value;
    }
    params
}

#[cfg(test)]
mod tests;
