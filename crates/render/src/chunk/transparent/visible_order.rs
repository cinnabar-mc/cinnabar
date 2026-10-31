//! Visible sub-chunks with transparent models or water, kept for the transparent view from
//! Bevy's per-frame visibility diffs and the allocation and mesh changes observers report, so
//! model preparation and the transparent queue never rescan every visible sub-chunk.
//!
//! Entries keep Bevy's visible order, main entity order, on which model preparation's stable
//! identity ties and the queue's choice among duplicate keys depend.
use crate::chunk::*;
use bevy::ecs::{
    change_detection::{DetectChanges, Ref},
    entity::EntityHashMap,
    lifecycle::{Insert, Remove},
    observer::On,
};
use bevy::render::view::RenderVisibleEntitiesClass;
use render_scene::ChunkRenderInstance;

/// Past this many unconsumed changes the next update rebuilds instead of applying each.
const MAX_LISTED_CHANGES: usize = 4096;

/// One visible sub-chunk with transparent water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct VisibleWater {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) main: MainEntity,
    pub(in crate::chunk) order_independent: bool,
}

/// One visible sub-chunk whose transparent models draw directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct VisibleModel {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) main: MainEntity,
}

/// What one visible allocation contributes to the transparent passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransparentChunk {
    entity: Entity,
    key: SubChunkKey,
    /// Carries both transparent model streams, so model preparation sorts it.
    sorted_models: bool,
    /// Draws its transparent models directly.
    drawn_models: bool,
    /// The tint table its direct water draw was built under, when it has one.
    water_tint: Option<ChunkBiomeTintIdentity>,
    order_independent: bool,
}

impl TransparentChunk {
    /// `allocation`'s transparent content, or `None` when it has neither models nor water.
    fn new(entity: Entity, allocation: &GpuChunkAllocation) -> Option<Self> {
        let chunk = Self {
            entity,
            key: allocation.key,
            sorted_models: has_transparent_model_streams(allocation),
            drawn_models: transparent_model_direct_draw_command(allocation).is_some(),
            water_tint: transparent_liquid_direct_draw_command(allocation)
                .map(|_| allocation.tint_identity),
            order_independent: allocation.order_independent_liquid,
        };
        (chunk.sorted_models || chunk.drawn_models || chunk.water_tint.is_some()).then_some(chunk)
    }
}

/// Bevy's chunk visibility for the transparent view this frame.
pub(in crate::chunk) struct ViewVisibility<'a> {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) class: Option<&'a RenderVisibleEntitiesClass>,
    /// Bevy refreshed the class since the last update, so its diffs describe this frame.
    pub(in crate::chunk) refreshed: bool,
}

/// The transparent view's visible sub-chunks with transparent models or water.
#[derive(Resource, Default)]
pub(in crate::chunk) struct VisibleTransparentChunks {
    view: Option<Entity>,
    /// Length of Bevy's visible list the entries were last reconciled with.
    visible_len: usize,
    entries: BTreeMap<MainEntity, TransparentChunk>,
    /// The main entity of each entry's render entity.
    mains: EntityHashMap<MainEntity>,
    /// Render entities whose allocation or mesh was inserted or removed since the last update.
    changed: Vec<Entity>,
    /// More changes arrived than listed; the next update rebuilds.
    rebuild: bool,
    /// Bumped whenever the visible model allocations, their meshes or their order may differ.
    model_revision: u64,
    /// The tint table `water` was filtered for; `None` while the lists are stale.
    lists_tint: Option<ChunkBiomeTintIdentity>,
    models: Vec<VisibleModel>,
    water: Vec<VisibleWater>,
}

impl VisibleTransparentChunks {
    /// The view the entries describe.
    pub(in crate::chunk) const fn view(&self) -> Option<Entity> {
        self.view
    }

    /// Changes whenever [`Self::sorted_models`] or the meshes of its entities may differ.
    pub(in crate::chunk) const fn model_revision(&self) -> u64 {
        self.model_revision
    }

    /// Visible entities whose allocation carries both transparent model streams, in Bevy's
    /// visible order.
    pub(in crate::chunk) fn sorted_models(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entries
            .values()
            .filter(|chunk| chunk.sorted_models)
            .map(|chunk| chunk.entity)
    }

    /// Visible directly drawn transparent models in key order, one per key: the key's last
    /// entity in Bevy's visible order.
    pub(in crate::chunk) fn models(&self) -> &[VisibleModel] {
        &self.models
    }

    /// Visible direct water drawn under the active tint table, in key then entity order.
    pub(in crate::chunk) fn water(&self) -> &[VisibleWater] {
        &self.water
    }

    /// Notes that `entity`'s allocation or mesh was inserted or removed.
    fn record_change(&mut self, entity: Entity) {
        if self.rebuild {
            return;
        }
        if self.changed.len() >= MAX_LISTED_CHANGES {
            self.changed.clear();
            self.rebuild = true;
        } else {
            self.changed.push(entity);
        }
    }

    /// Brings the entries up to date with `view`'s visibility and the recorded changes, then
    /// the queue's lists with the active `tint` table.
    pub(in crate::chunk) fn update<'a>(
        &mut self,
        view: Option<ViewVisibility<'_>>,
        allocation: impl Fn(Entity) -> Option<&'a GpuChunkAllocation>,
        main: impl Fn(Entity) -> Option<MainEntity>,
        tint: ChunkBiomeTintIdentity,
    ) {
        let mut changed = std::mem::take(&mut self.changed);
        let mut rebuild = std::mem::take(&mut self.rebuild);
        match view {
            None => self.clear(),
            Some(view) => {
                // Bevy reports no diffs for GPU-culled entities; the client spawns none.
                rebuild |= self.view != Some(view.entity)
                    || view
                        .class
                        .is_some_and(|class| !class.entities_gpu_culling.is_empty());
                if !rebuild && view.refreshed {
                    rebuild = !self.apply_visibility_diff(view.class, &allocation);
                }
                if !rebuild {
                    rebuild = !changed
                        .iter()
                        .all(|&entity| self.apply_change(entity, view.class, &allocation, &main));
                }
                if rebuild {
                    self.rebuild_view(view, &allocation);
                }
                self.refresh_lists(tint);
            }
        }
        changed.clear();
        self.changed = changed;
    }

    /// Forgets the view and every entry.
    fn clear(&mut self) {
        self.view = None;
        self.visible_len = 0;
        self.entries.clear();
        self.mains.clear();
        self.models.clear();
        self.water.clear();
        self.lists_tint = None;
        self.model_revision = self.model_revision.wrapping_add(1);
    }

    /// Applies Bevy's added and removed entities. Returns false when they do not account for
    /// the visible list's new length, so the entries must be rebuilt.
    fn apply_visibility_diff<'a>(
        &mut self,
        class: Option<&RenderVisibleEntitiesClass>,
        allocation: &impl Fn(Entity) -> Option<&'a GpuChunkAllocation>,
    ) -> bool {
        let (len, added, removed) = class.map_or((0, &[][..], &[][..]), |class| {
            (
                class.entities_cpu_culling.len(),
                &class.added_entities[..],
                &class.removed_entities[..],
            )
        });
        if self.visible_len + added.len() != len + removed.len() {
            return false;
        }
        for (_, main) in removed {
            self.remove(*main);
        }
        for &(entity, main) in added {
            self.upsert(entity, main, allocation(entity));
        }
        self.visible_len = len;
        true
    }

    /// Re-evaluates one changed render entity. Returns false when it might be visible but has
    /// no main entity to look it up by.
    fn apply_change<'a>(
        &mut self,
        entity: Entity,
        class: Option<&RenderVisibleEntitiesClass>,
        allocation: &impl Fn(Entity) -> Option<&'a GpuChunkAllocation>,
        main: &impl Fn(Entity) -> Option<MainEntity>,
    ) -> bool {
        if let Some(&main) = self.mains.get(&entity) {
            let sorted = self.entries[&main].sorted_models;
            self.upsert(entity, main, allocation(entity));
            if sorted {
                // Its mesh may have changed under an unchanged allocation.
                self.model_revision = self.model_revision.wrapping_add(1);
            }
            return true;
        }
        let Some(current) = allocation(entity) else {
            return true;
        };
        if TransparentChunk::new(entity, current).is_none() {
            return true;
        }
        let Some(main) = main(entity) else {
            return false;
        };
        if class.is_some_and(|class| class.entity_pair_is_visible(entity, main)) {
            self.upsert(entity, main, Some(current));
        }
        true
    }

    /// Sets `main`'s entry to `entity`'s transparent content under `allocation`.
    fn upsert(
        &mut self,
        entity: Entity,
        main: MainEntity,
        allocation: Option<&GpuChunkAllocation>,
    ) {
        if let Some(&other) = self.mains.get(&entity)
            && other != main
        {
            self.remove(other);
        }
        let next = allocation.and_then(|allocation| TransparentChunk::new(entity, allocation));
        let previous = self.entries.get(&main).copied();
        if previous == next {
            return;
        }
        if let Some(previous) = previous {
            self.mains.remove(&previous.entity);
        }
        match next {
            Some(next) => {
                self.entries.insert(main, next);
                self.mains.insert(entity, main);
            }
            None => {
                self.entries.remove(&main);
            }
        }
        self.changed_entry(previous, next);
    }

    /// Drops `main`'s entry, if it has one.
    fn remove(&mut self, main: MainEntity) {
        if let Some(previous) = self.entries.remove(&main) {
            self.mains.remove(&previous.entity);
            self.changed_entry(Some(previous), None);
        }
    }

    /// Stales the lists, and the model revision when either side sorts models.
    fn changed_entry(
        &mut self,
        previous: Option<TransparentChunk>,
        next: Option<TransparentChunk>,
    ) {
        self.lists_tint = None;
        if previous
            .into_iter()
            .chain(next)
            .any(|chunk| chunk.sorted_models)
        {
            self.model_revision = self.model_revision.wrapping_add(1);
        }
    }

    /// Rebuilds every entry from `view`'s visible list.
    fn rebuild_view<'a>(
        &mut self,
        view: ViewVisibility<'_>,
        allocation: &impl Fn(Entity) -> Option<&'a GpuChunkAllocation>,
    ) {
        self.view = Some(view.entity);
        self.entries.clear();
        self.mains.clear();
        self.visible_len = 0;
        if let Some(class) = view.class {
            for (&entity, &main) in class.iter_visible() {
                if let Some(chunk) = allocation(entity)
                    .and_then(|allocation| TransparentChunk::new(entity, allocation))
                {
                    self.entries.insert(main, chunk);
                    self.mains.insert(entity, main);
                }
            }
            self.visible_len = class.entities_cpu_culling.len();
        }
        self.lists_tint = None;
        self.model_revision = self.model_revision.wrapping_add(1);
    }

    /// Rebuilds the queue's lists after an entry or the tint table changed.
    fn refresh_lists(&mut self, tint: ChunkBiomeTintIdentity) {
        if self.lists_tint == Some(tint) {
            return;
        }
        self.models.clear();
        self.models.extend(
            self.entries
                .iter()
                .filter(|(_, chunk)| chunk.drawn_models)
                .map(|(&main, chunk)| VisibleModel {
                    key: chunk.key,
                    entity: chunk.entity,
                    main,
                }),
        );
        // Key order with the last visible entity of each key, as a map insert would keep.
        self.models.reverse();
        self.models.sort_by_key(|model| model.key);
        self.models.dedup_by_key(|model| model.key);
        // Like opaque terrain, water waits for its re-upload under a new tint table.
        self.water.clear();
        self.water
            .extend(self.entries.iter().filter_map(|(&main, chunk)| {
                chunk
                    .water_tint
                    .filter(|&water_tint| chunk_tint_identity_is_active(water_tint, tint))
                    .map(|_| VisibleWater {
                        key: chunk.key,
                        entity: chunk.entity,
                        main,
                        order_independent: chunk.order_independent,
                    })
            }));
        // Equal phase distances keep insertion order, so water enters in key order.
        self.water
            .sort_unstable_by_key(|water| (water.key, water.entity));
        self.lists_tint = Some(tint);
    }
}

/// Reports every chunk allocation or mesh insertion and removal in `world` to the tracker.
pub(in crate::chunk) fn install_visible_transparent_tracking(world: &mut World) {
    world.init_resource::<VisibleTransparentChunks>();
    world.add_observer(
        |insert: On<Insert<(GpuChunkAllocation, ChunkRenderInstance)>>,
         mut chunks: ResMut<VisibleTransparentChunks>| {
            chunks.record_change(insert.entity);
        },
    );
    world.add_observer(
        |remove: On<Remove<(GpuChunkAllocation, ChunkRenderInstance)>>,
         mut chunks: ResMut<VisibleTransparentChunks>| {
            chunks.record_change(remove.entity);
        },
    );
}

/// Updates the visible transparent sub-chunks of the transparent view for this frame.
pub(in crate::chunk) fn track_visible_transparent_chunks(
    mut chunks: ResMut<VisibleTransparentChunks>,
    runtime: Res<TransparentSortRuntime>,
    views: Query<Ref<RenderVisibleEntities>, With<bevy::render::camera::ExtractedCamera>>,
    allocations: Query<&GpuChunkAllocation>,
    mains: Query<&MainEntity>,
    biome_tints: Res<ChunkBiomeTints>,
) {
    let view = runtime
        .view_entity
        .and_then(|entity| views.get(entity).ok().map(|visible| (entity, visible)));
    chunks.update(
        view.as_ref().map(|(entity, visible)| ViewVisibility {
            entity: *entity,
            class: visible.get::<ChunkRenderInstance>(),
            refreshed: visible.is_changed(),
        }),
        |entity| allocations.get(entity).ok(),
        |entity| mains.get(entity).ok().copied(),
        biome_tints.table_identity(),
    );
}

/// Visits `ordered` water once per key, choosing among duplicates the entity that
/// `residents` holds as the key's current upload, else the first.
pub(in crate::chunk) fn each_visible_key(
    ordered: &[VisibleWater],
    residents: &TransparentLiquidResidents,
) -> impl Iterator<Item = VisibleWater> {
    ordered
        .chunk_by(|left, right| left.key == right.key)
        .map(move |same_key| match same_key {
            [only] => *only,
            duplicates => {
                let resident = residents
                    .get(duplicates[0].key)
                    .map(|resident| resident.entity);
                *duplicates
                    .iter()
                    .find(|water| Some(water.entity) == resident)
                    .unwrap_or(&duplicates[0])
            }
        })
}

#[cfg(test)]
mod tests;
