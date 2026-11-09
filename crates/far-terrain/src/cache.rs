//! One client's far-terrain cache: the active namespace in memory, persisted per region.
//!
//! Every summary carries a version stamped when it was taken, from a counter that only rises
//! across runs. Memory, waiting writes, writes in flight, region files and disk reads all keep
//! the higher version of a column, so an older summary never replaces a newer one wherever and
//! however late it arrives.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use web_time::Instant;

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, FarTerrainConfig, NamespaceKey, ServerKey,
    SummaryBudget, SummaryStore, Versioned, WorldKey,
    config::{
        MAX_PENDING_WRITE_REGIONS, MAX_UNRESOLVED_COLUMNS, REGION_FLUSH_AFTER,
        REGION_FLUSH_COLUMNS, WORLD_DIVERGED_COLUMNS, WORLD_PROBE_COLUMNS, WORLD_PROBE_SIMILARITY,
        WORLD_PROBE_TIMEOUT, WORLD_VERIFY_COLUMNS,
    },
    disk::{DiskBounds, DiskCache, ResolutionIntent},
    identity::surface_similarity,
    region::{REGION_SIDE, RegionPos, region_index},
};

mod completion;
use completion::PendingEvent;

#[cfg(test)]
mod admission_tests;

#[derive(Debug)]
struct PendingRegion {
    /// Age of edits not yet submitted; absent when every retained version was submitted.
    age: Option<BatchAge>,
    /// Payloads are kept here, not looked up at flush, so memory eviction cannot lose them.
    columns: BTreeMap<ColumnPos, Versioned>,
    in_flight: bool,
}

/// Retirement work can be immediately due without inventing a clock reading.
#[derive(Clone, Copy, Debug)]
enum BatchAge {
    Immediate,
    Since(Instant),
}

/// A world just left, which late summaries still reach.
struct Retired {
    identity: RetiredIdentity,
    columns: Vec<(ColumnPos, Versioned)>,
}

/// The namespace or deferred identity decision belonging to one retired visit.
enum RetiredIdentity {
    Resolved(NamespaceKey),
    /// Keeps the identity decision for a visit left before the worker resolved its namespace.
    Unresolved {
        world: WorldKey,
        /// Its visit, so every late batch resolves to the same epoch.
        entry: u64,
        intent: ResolutionIntent,
        probes: Vec<(ColumnPos, ColumnSummary)>,
    },
}

impl Retired {
    /// Accepts unkeyed late columns only while this is the most recently ended visit.
    fn accepts_late(&self, dimension: i32, current_entry: u64) -> bool {
        match &self.identity {
            RetiredIdentity::Resolved(namespace) => namespace.dimension == dimension,
            RetiredIdentity::Unresolved { world, entry, .. } => {
                world.dimension == dimension && entry.checked_add(1) == Some(current_entry)
            }
        }
    }

    /// A deferred migration must reach the worker before another retirement can replace it.
    fn migration_pending(&self) -> bool {
        matches!(
            self.identity,
            RetiredIdentity::Unresolved {
                intent: ResolutionIntent::Migrate { .. },
                ..
            }
        )
    }

    /// True while retained columns or a non-default identity decision need admission.
    fn needs_flush(&self) -> bool {
        !self.columns.is_empty()
            || matches!(self.identity, RetiredIdentity::Unresolved { intent, .. } if intent != ResolutionIntent::Probed)
    }
}

/// A session whose final summaries are still being extracted, from
/// [`FarTerrainCache::retire_session`]; hand it back to [`FarTerrainCache::persist`]. Its
/// summaries carry the version stamped at retirement, so anything taken later wins.
#[derive(Clone, Debug)]
#[must_use]
pub struct RetiringSession {
    world: WorldKey,
    /// The visit retiring, whose namespace its summaries share once resolved.
    entry: u64,
    intent: ResolutionIntent,
    namespace: Option<NamespaceKey>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    version: u64,
}

impl RetiringSession {
    /// Keeps the first non-empty retirement probes before persistence is split into batches.
    pub fn add_probe(&mut self, pos: ColumnPos, summary: &EncodedSummary) {
        if self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
    }

    #[must_use]
    pub const fn dimension(&self) -> i32 {
        self.world.dimension
    }
}

/// Summaries of the world the client is in, with reads and batched writes on an IO thread.
///
/// A world's namespace is resolved from its first non-empty columns, so a server that reuses
/// one level identity for different maps gets a fresh namespace instead of mixed terrain.
pub struct FarTerrainCache {
    /// Started on the first world entered, so a client that never uses far terrain runs no
    /// IO thread.
    disk: Option<DiskCache>,
    disk_root: Option<(PathBuf, DiskBounds)>,
    store: SummaryStore,
    world: Option<WorldKey>,
    namespace: Option<NamespaceKey>,
    /// The world left by the last world change; late summaries for it go straight to disk.
    retired: Option<Retired>,
    /// Columns shown before the namespace resolved, written once it does.
    unresolved: Vec<(ColumnPos, Versioned)>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    resolving: bool,
    /// Identity intent survives refused work and follows the visit into retirement.
    intent: ResolutionIntent,
    /// When this visit's first column arrived, for [`WORLD_PROBE_TIMEOUT`].
    pending_since: Option<Instant>,
    /// One owner keeps each region's newest unacknowledged columns through submission.
    pending: BTreeMap<(NamespaceKey, RegionPos), PendingRegion>,
    pending_event: Option<PendingEvent>,
    requested: HashSet<RegionPos>,
    /// Regions read again after memory dropped some of their columns, once per focus.
    reread: HashSet<RegionPos>,
    focus: Option<(ColumnPos, i32)>,
    /// Bumped on every world change, so a resolution for an earlier visit is ignored.
    entry: u64,
    next_version: u64,
    /// Older cached versions are unsafe once pressure discards a newer pending payload.
    /// One conservative watermark avoids retaining an unbounded set of lost columns.
    discarded_through: u64,
    /// First version stamped this visit; anything older in memory came from disk.
    visit_start: u64,
    /// Cached columns compared with live ones this visit, and how many differed.
    compared: u32,
    mismatched: u32,
}

impl FarTerrainCache {
    /// A cache persisted under `disk_root`, or memory-only without one. `version_base` must
    /// exceed every version an earlier run stamped, as a clock reading in microseconds does.
    #[must_use]
    pub fn new(config: FarTerrainConfig, disk_root: Option<PathBuf>, version_base: u64) -> Self {
        // An invalid caller clock cannot seed persistence or overflow a later live stamp.
        let (disk_root, version_base) = if version_base >= crate::region::MAX_VERSION {
            (None, 0)
        } else {
            (disk_root, version_base)
        };
        let bounds = DiskBounds {
            server_cap_bytes: config.server_disk_cap_bytes,
            global_cap_bytes: config.global_disk_cap_bytes,
        };
        Self {
            disk: None,
            disk_root: disk_root.map(|root| (root, bounds)),
            store: SummaryStore::new(config.memory_budget_bytes),
            world: None,
            namespace: None,
            retired: None,
            unresolved: Vec::new(),
            probes: Vec::new(),
            resolving: false,
            intent: ResolutionIntent::Probed,
            pending_since: None,
            pending: BTreeMap::new(),
            pending_event: None,
            requested: HashSet::new(),
            reread: HashSet::new(),
            focus: None,
            entry: 0,
            next_version: version_base,
            discarded_through: 0,
            visit_start: version_base,
            compared: 0,
            mismatched: 0,
        }
    }

    /// Switches to `dimension` of the level `level` on the server at `destination`.
    /// Anything else, including a transfer, flushes and leaves the previous namespace.
    pub fn enter_world(&mut self, destination: &str, level: u64, dimension: i32) {
        let world = WorldKey {
            server: ServerKey::from_destination(destination),
            dimension,
            level,
            resources: [0; 32],
        };
        if self.world != Some(world) {
            self.begin_visit(destination, level, dimension);
        }
    }

    /// Starts a fresh visit to `dimension` of `level` on `destination`, even of the world
    /// already entered, as after a transfer: nothing from the previous visit stays shown,
    /// and its namespace is resolved again from new probes.
    pub fn begin_visit(&mut self, destination: &str, level: u64, dimension: i32) {
        self.begin_visit_with_resources(destination, level, dimension, [0; 32]);
    }

    /// Starts a visit whose disk and memory summaries belong to these resource inputs.
    pub fn begin_visit_with_resources(
        &mut self,
        destination: &str,
        level: u64,
        dimension: i32,
        resources: [u8; 32],
    ) {
        self.end_entry();
        if let Some((root, bounds)) = self.disk_root.take() {
            self.disk = DiskCache::spawn(root, bounds);
        }
        self.world = Some(WorldKey {
            server: ServerKey::from_destination(destination),
            dimension,
            level,
            resources,
        });
    }

    /// Starts a visit without a stable world identity. It always reserves a new epoch,
    /// so matching lobby columns cannot enable reuse.
    pub fn begin_uncertain_visit(&mut self, destination: &str, dimension: i32) {
        self.begin_uncertain_visit_with_resources(destination, dimension, [0; 32]);
    }

    /// Reserves a fresh world epoch while keeping other resource generations separate.
    pub fn begin_uncertain_visit_with_resources(
        &mut self,
        destination: &str,
        dimension: i32,
        resources: [u8; 32],
    ) {
        self.begin_visit_with_resources(destination, 0, dimension, resources);
        self.intent = ResolutionIntent::Fresh;
        self.start_resolving();
    }

    /// Leaves the current world, as on disconnect, after scheduling its retained writes.
    pub fn leave_world(&mut self) {
        self.end_entry();
        self.world = None;
    }

    /// Flushes pending writes and keeps an unresolved visit's identity decision for retirement.
    fn end_entry(&mut self) {
        self.flush_all();
        self.flush_retired();
        let identity = match (self.namespace.take(), self.world) {
            (Some(namespace), _) => Some(RetiredIdentity::Resolved(namespace)),
            (None, Some(world)) => Some(RetiredIdentity::Unresolved {
                world,
                entry: self.entry,
                intent: self.intent,
                probes: std::mem::take(&mut self.probes),
            }),
            (None, None) => None,
        };
        let retired = identity.map(|identity| Retired {
            identity,
            columns: std::mem::take(&mut self.unresolved),
        });
        self.replace_retired(retired);
        self.flush_retired();
        self.entry += 1;
        self.visit_start = self.next_version + 1;
        self.compared = 0;
        self.mismatched = 0;
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.intent = ResolutionIntent::Probed;
        self.pending_since = None;
        self.requested.clear();
        self.reread.clear();
    }

    /// Preserves a deferred migration and invalidates every payload a visit change discards.
    fn replace_retired(&mut self, next: Option<Retired>) {
        let discarded = if self
            .retired
            .as_ref()
            .is_some_and(Retired::migration_pending)
        {
            next
        } else {
            std::mem::replace(&mut self.retired, next)
        };
        if let Some(retired) = discarded {
            self.discarded_through = retired
                .columns
                .iter()
                .fold(self.discarded_through, |version, (_, column)| {
                    version.max(column.version)
                });
        }
    }

    /// Retries retired identity work and waiting columns without blocking the frame thread.
    fn flush_retired(&mut self) {
        let Some(mut retired) = self.retired.take() else {
            return;
        };
        match &mut retired.identity {
            RetiredIdentity::Resolved(namespace) => {
                while let Some((pos, column)) = retired.columns.last().cloned() {
                    if !self.try_queue_write(*namespace, pos, &column, None) {
                        break;
                    }
                    retired.columns.pop();
                }
            }
            RetiredIdentity::Unresolved {
                world,
                entry,
                intent,
                probes,
            } => {
                if let Some(disk) = &self.disk
                    && (!retired.columns.is_empty() || *intent != ResolutionIntent::Probed)
                {
                    // Late columns probe too, so a world left before its first summary is still
                    // compared against cached maps rather than reusing the newest blindly.
                    add_probes(probes, &retired.columns);
                    if disk.resolve_and_write(
                        *world,
                        *entry,
                        *intent,
                        probes.clone(),
                        retired.columns.clone(),
                    ) {
                        retired.columns.clear();
                        *intent = ResolutionIntent::Probed;
                    }
                }
            }
        }
        self.retired = Some(retired);
    }

    /// Records a column's encoded summary as the newest of it. Columns show at once and are
    /// written once the world's namespace is resolved.
    pub fn admit(&mut self, dimension: i32, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        let column = Versioned {
            version: self.stamp(),
            summary,
        };
        if self
            .retired
            .as_ref()
            .is_some_and(|retired| retired.accepts_late(dimension, self.entry))
            && self.world.is_none_or(|world| world.dimension != dimension)
        {
            match self
                .retired
                .as_mut()
                .map(|retired| (&mut retired.identity, &mut retired.columns))
            {
                Some((RetiredIdentity::Resolved(namespace), _)) => {
                    let namespace = *namespace;
                    self.queue_write(namespace, pos, column, now);
                }
                Some((RetiredIdentity::Unresolved { .. }, columns)) if self.disk.is_some() => {
                    retain_unresolved(columns, &mut self.discarded_through, pos, column);
                }
                Some((RetiredIdentity::Unresolved { .. }, _)) | None => {}
            }
            return;
        }
        if self.world.is_none_or(|world| world.dimension != dimension) {
            return;
        }
        if let Some(namespace) = self.namespace {
            if self.verify(pos, &column) {
                self.diverge();
                self.admit(dimension, pos, column.summary, now);
                return;
            }
            self.store.admit_versioned(pos, column.clone());
            self.queue_write(namespace, pos, column, now);
            return;
        }
        self.pending_since.get_or_insert(now);
        if !self.resolving
            && self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&column.summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
        if self.disk.is_some() {
            retain_unresolved(
                &mut self.unresolved,
                &mut self.discarded_through,
                pos,
                column.clone(),
            );
        }
        self.store.admit_versioned(pos, column);
        if !self.resolving && self.probes.len() == WORLD_PROBE_COLUMNS {
            self.start_resolving();
        }
    }

    /// Loads cached regions within `radius` chunks of `center` once each.
    pub fn set_focus(&mut self, center: ColumnPos, radius: i32) {
        if self.focus == Some((center, radius)) {
            return;
        }
        self.focus = Some((center, radius));
        self.reread.clear();
        self.request_focus();
    }

    /// Applies finished disk work, resolves a sparse world after [`WORLD_PROBE_TIMEOUT`] and
    /// writes regions whose changes are due.
    pub fn poll(&mut self, now: Instant) {
        self.poll_budgeted(now, &mut SummaryBudget::default());
    }

    /// Applies a bounded share of completed disk columns, retaining partial events in order.
    pub fn poll_budgeted(&mut self, now: Instant, budget: &mut SummaryBudget) {
        self.apply_completions(Some(now), budget);
        if self.intent != ResolutionIntent::Probed && self.namespace.is_none() && !self.resolving {
            self.start_resolving();
        }
        if self.namespace.is_none()
            && !self.resolving
            && self
                .pending_since
                .is_some_and(|since| now.saturating_duration_since(since) >= WORLD_PROBE_TIMEOUT)
        {
            self.start_resolving();
        }
        self.flush_retired();
        self.request_focus();
        let evicted = self.store.take_evicted();
        self.reread_evicted(&evicted);
        let due: Vec<(NamespaceKey, RegionPos)> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                pending.columns.len() >= REGION_FLUSH_COLUMNS
                    || pending.age.is_some_and(|age| match age {
                        BatchAge::Immediate => true,
                        BatchAge::Since(since) => {
                            now.saturating_duration_since(since) >= REGION_FLUSH_AFTER
                        }
                    })
            })
            .map(|(region, _)| *region)
            .collect();
        for region in due {
            self.flush(region);
        }
    }

    /// Submits retained regions as IO admission allows; refused work stays queued.
    pub fn flush_all(&mut self) {
        let regions: Vec<(NamespaceKey, RegionPos)> = self.pending.keys().copied().collect();
        for region in regions {
            self.flush(region);
        }
    }

    /// Starts a session's retirement while its final summaries are extracted, stamping the
    /// version they will carry; `None` outside a world.
    pub fn retire_session(&mut self) -> Option<RetiringSession> {
        let world = self.world?;
        let version = self.stamp();
        Some(RetiringSession {
            world,
            entry: self.entry,
            intent: self.intent,
            namespace: self.namespace,
            probes: self.probes.clone(),
            version,
        })
    }

    /// Writes a retired session's final summaries, and shows them when their namespace is
    /// active; wherever a newer version exists it is kept.
    pub fn persist(
        &mut self,
        session: RetiringSession,
        columns: impl IntoIterator<Item = (ColumnPos, EncodedSummary)>,
        now: Instant,
    ) {
        // Memory-only visits cannot establish a shared namespace with an earlier map.
        if self.disk.is_none() && session.entry != self.entry {
            return;
        }
        let RetiringSession {
            world,
            entry,
            intent,
            namespace,
            mut probes,
            version,
        } = session;
        let mut unresolved = Vec::new();
        for (pos, summary) in columns {
            let column = Versioned { version, summary };
            if version <= self.discarded_through
                || namespace.is_some_and(|namespace| self.newer_queued(namespace, pos, version))
            {
                continue;
            }
            if namespace.is_some() && namespace == self.namespace {
                self.store.admit_versioned(pos, column.clone());
            }
            match namespace {
                Some(namespace) => self.queue_write(namespace, pos, column, now),
                None if self.disk.is_some() => {
                    retain_unresolved(&mut unresolved, &mut self.discarded_through, pos, column);
                }
                None => {}
            }
        }
        if let Some(disk) = &self.disk
            && !unresolved.is_empty()
        {
            add_probes(&mut probes, &unresolved);
            if !disk.resolve_and_write(world, entry, intent, probes, unresolved) {
                self.discarded_through = self.discarded_through.max(version);
            }
        }
    }

    /// Resizes the in-memory summary budget, as when the horizon grows or shrinks.
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.store.set_budget(bytes);
    }

    /// Waits until the IO thread has finished commands already submitted to it.
    pub fn sync(&self) {
        if let Some(disk) = &self.disk {
            disk.sync();
        }
    }

    /// Drains retained payloads and acknowledgements before leaving, without a frame budget.
    pub fn finish(&mut self) {
        self.focus = None;
        while self.disk.is_some() {
            if self.world.is_some() && self.namespace.is_none() && !self.resolving {
                self.start_resolving();
            }
            self.flush_retired();
            self.flush_all();
            if !self.disk.as_ref().expect("checked above").sync() {
                break;
            }
            self.apply_completions(None, &mut SummaryBudget::new(usize::MAX, usize::MAX));
            if self.pending.is_empty()
                && self.unresolved.is_empty()
                && self.pending_event.is_none()
                && self
                    .retired
                    .as_ref()
                    .is_none_or(|retired| !retired.needs_flush())
                && (self.world.is_none() || self.namespace.is_some())
            {
                break;
            }
        }
        self.leave_world();
    }

    /// The active namespace, once its world identity is resolved.
    #[must_use]
    pub const fn namespace(&self) -> Option<NamespaceKey> {
        self.namespace
    }

    /// Summaries of the current world, including those shown before its namespace resolves.
    pub fn store(&mut self) -> &mut SummaryStore {
        &mut self.store
    }

    /// True when a version of `pos` at least `version` waits to be written or is being written.
    fn newer_queued(&self, namespace: NamespaceKey, pos: ColumnPos, version: u64) -> bool {
        self.queued(namespace, pos)
            .is_some_and(|queued| queued.version >= version)
    }

    /// The newest pending write for a column, whether waiting or already on the IO thread.
    fn queued(&self, namespace: NamespaceKey, pos: ColumnPos) -> Option<&Versioned> {
        let key = (namespace, RegionPos::of(pos));
        self.pending
            .get(&key)
            .and_then(|pending| pending.columns.get(&pos))
    }

    /// Compares a live column with the cached one it replaces, for the first
    /// [`WORLD_VERIFY_COLUMNS`] of a visit; true once enough differ that the cache must hold
    /// another map under this identity.
    fn verify(&mut self, pos: ColumnPos, live: &Versioned) -> bool {
        if self.compared >= WORLD_VERIFY_COLUMNS
            || self
                .store
                .version(pos)
                .is_none_or(|version| version >= self.visit_start)
        {
            return false;
        }
        let (Some(Ok(cached)), Ok(live)) = (
            self.store
                .encoded(pos)
                .map(|cached| ColumnSummary::decode(&cached.bytes)),
            ColumnSummary::decode(&live.summary.bytes),
        ) else {
            return false;
        };
        self.compared += 1;
        if surface_similarity(&cached, &live) < WORLD_PROBE_SIMILARITY {
            self.mismatched += 1;
        }
        self.mismatched >= WORLD_DIVERGED_COLUMNS && self.mismatched * 4 >= self.compared
    }

    /// Drops what the cache showed for this visit and starts a new epoch for it, keeping
    /// every live column, which is written there once it resolves.
    fn diverge(&mut self) {
        let (Some(_), Some(namespace), Some(_)) = (self.world, self.namespace.take(), &self.disk)
        else {
            return;
        };
        self.store.retain_since(self.visit_start);
        let keys: Vec<(NamespaceKey, RegionPos)> = self
            .pending
            .keys()
            .copied()
            .filter(|(written, _)| *written == namespace)
            .collect();
        for key in keys {
            let Some(pending) = self.pending.get_mut(&key) else {
                continue;
            };
            let visit = self.visit_start;
            let (live, older): (BTreeMap<_, _>, BTreeMap<_, _>) =
                std::mem::take(&mut pending.columns)
                    .into_iter()
                    .partition(|(_, column)| column.version >= visit);
            pending.columns = older;
            if pending.columns.is_empty() && !pending.in_flight {
                self.pending.remove(&key);
            }
            for (pos, column) in live {
                retain_unresolved(
                    &mut self.unresolved,
                    &mut self.discarded_through,
                    pos,
                    column,
                );
            }
        }
        self.entry += 1;
        self.resolving = false;
        self.intent = ResolutionIntent::Migrate {
            from: namespace,
            since: self.visit_start,
        };
        self.requested.clear();
        self.reread.clear();
        self.compared = WORLD_VERIFY_COLUMNS;
        // Columns of this visit already written, or in flight before this, move on the IO
        // thread, which handles commands in order.
        self.start_resolving();
    }

    fn stamp(&mut self) -> u64 {
        if let Some(disk) = &self.disk {
            self.next_version = self.next_version.max(disk.observed_version());
        }
        if self.next_version >= crate::region::MAX_VERSION {
            // Clock exhaustion invalidates optional cache state, never wraps its ordering.
            self.disk = None;
            self.disk_root = None;
            self.store.clear();
            self.pending.clear();
            self.unresolved.clear();
            self.retired = None;
            self.pending_event = None;
            // Outstanding retirement handles belong to the abandoned version ordering.
            self.entry += 1;
            self.namespace = self.world.map(|world| world.namespace(0));
            self.resolving = false;
            self.intent = ResolutionIntent::Probed;
            self.requested.clear();
            self.reread.clear();
            self.next_version = 0;
            self.discarded_through = 0;
            self.visit_start = 1;
        }
        self.next_version += 1;
        self.next_version
    }

    fn start_resolving(&mut self) {
        if self
            .retired
            .as_ref()
            .is_some_and(Retired::migration_pending)
        {
            self.flush_retired();
            if self
                .retired
                .as_ref()
                .is_some_and(Retired::migration_pending)
            {
                return;
            }
        }
        let Some(world) = self.world else {
            return;
        };
        self.resolving = match &self.disk {
            Some(disk) => disk.resolve(world, self.entry, self.intent, self.probes.clone()),
            None => {
                self.resolved(world, 0);
                true
            }
        };
    }

    fn resolved(&mut self, world: WorldKey, epoch: u32) {
        if self.world != Some(world) || self.namespace.is_some() {
            return;
        }
        let namespace = world.namespace(epoch);
        self.namespace = Some(namespace);
        if self.disk.is_none() {
            self.unresolved.clear();
        }
        self.request_focus();
    }

    /// Queues `column` for writing unless a newer version of it already waits.
    fn queue_write(
        &mut self,
        namespace: NamespaceKey,
        pos: ColumnPos,
        column: Versioned,
        now: Instant,
    ) {
        if self.disk.is_some() && !self.try_queue_write(namespace, pos, &column, Some(now)) {
            self.discarded_through = self.discarded_through.max(column.version);
        }
    }

    /// Reserves one canonical writeback region, leaving refused retained payloads with callers.
    fn try_queue_write(
        &mut self,
        namespace: NamespaceKey,
        pos: ColumnPos,
        column: &Versioned,
        since: Option<Instant>,
    ) -> bool {
        if self.disk.is_none() {
            return false;
        }
        let key = (namespace, RegionPos::of(pos));
        if column.summary.bytes.len() > crate::summary::MAX_ENCODED_BYTES
            || (!self.pending.contains_key(&key) && self.pending.len() >= MAX_PENDING_WRITE_REGIONS)
        {
            return false;
        }
        let pending = self
            .pending
            .entry((namespace, RegionPos::of(pos)))
            .or_insert_with(|| PendingRegion {
                age: None,
                columns: BTreeMap::new(),
                in_flight: false,
            });
        if pending
            .columns
            .get(&pos)
            .is_none_or(|queued| queued.version < column.version)
        {
            pending.age = Some(match (pending.age, since) {
                (Some(BatchAge::Immediate), _) | (_, None) => BatchAge::Immediate,
                (Some(BatchAge::Since(first)), Some(_)) => BatchAge::Since(first),
                (None, Some(now)) => BatchAge::Since(now),
            });
            pending.columns.insert(pos, column.clone());
        }
        true
    }

    /// Submits each region once while its owner retains newer edits until acknowledgement.
    fn flush(&mut self, key: (NamespaceKey, RegionPos)) {
        let (Some(pending), Some(disk)) = (self.pending.get_mut(&key), &self.disk) else {
            return;
        };
        if pending.in_flight || pending.columns.is_empty() {
            return;
        }
        let columns = pending
            .columns
            .iter()
            .map(|(pos, column)| (region_index(*pos), column.clone()))
            .collect();
        if disk.write(key.0, key.1, columns) {
            pending.in_flight = true;
            pending.age = None;
        }
    }

    /// Reads regions again whose columns memory dropped while still in focus, once per
    /// focus, so a budget shortfall cannot turn into a read loop.
    fn reread_evicted(&mut self, evicted: &[ColumnPos]) {
        let Some((center, radius)) = self.focus else {
            return;
        };
        let mut forgot = false;
        for pos in evicted {
            let region = RegionPos::of(*pos);
            if (pos.x - center.x).abs() <= radius
                && (pos.z - center.z).abs() <= radius
                && self.reread.insert(region)
            {
                forgot |= self.requested.remove(&region);
            }
        }
        if forgot {
            self.request_focus();
        }
    }

    fn request_focus(&mut self) {
        let (Some(disk), Some(namespace), Some((center, radius))) =
            (&self.disk, self.namespace, self.focus)
        else {
            return;
        };
        let low = RegionPos::of(ColumnPos::new(center.x - radius, center.z - radius));
        let high = RegionPos::of(ColumnPos::new(center.x + radius, center.z + radius));
        // Forget regions well outside the focus so returning reloads them.
        let keep = radius / REGION_SIDE + 2;
        let centre = RegionPos::of(center);
        self.requested.retain(|region| {
            (region.x - centre.x).abs() <= keep && (region.z - centre.z).abs() <= keep
        });
        for x in low.x..=high.x {
            for z in low.z..=high.z {
                let region = RegionPos { x, z };
                if !self.requested.contains(&region) {
                    if !disk.load(namespace, region) {
                        return;
                    }
                    self.requested.insert(region);
                }
            }
        }
    }
}

impl Drop for FarTerrainCache {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Retains a bounded unresolved payload, remembering which older versions become unsafe.
fn retain_unresolved(
    columns: &mut Vec<(ColumnPos, Versioned)>,
    discarded_through: &mut u64,
    pos: ColumnPos,
    column: Versioned,
) {
    if columns.len() < MAX_UNRESOLVED_COLUMNS
        && column.summary.bytes.len() <= crate::summary::MAX_ENCODED_BYTES
    {
        columns.push((pos, column));
    } else {
        *discarded_through = (*discarded_through).max(column.version);
    }
}

/// Tops `probes` up to [`WORLD_PROBE_COLUMNS`] with the non-empty ones among `columns`.
fn add_probes(probes: &mut Vec<(ColumnPos, ColumnSummary)>, columns: &[(ColumnPos, Versioned)]) {
    for (pos, column) in columns {
        if probes.len() >= WORLD_PROBE_COLUMNS {
            return;
        }
        if let Ok(decoded) = ColumnSummary::decode(&column.summary.bytes)
            && !decoded.is_empty()
        {
            probes.push((*pos, decoded));
        }
    }
}
