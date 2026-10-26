//! CPU witnesses for the exact model-reference order already uploaded to the GPU.

use super::*;
use render_scene::ChunkRenderInstance;

#[derive(Debug)]
pub(in crate::chunk) struct TransparentModelDrawOrder {
    pub(in crate::chunk) identity: TransparentModelAllocationIdentity,
    pub(in crate::chunk) revision: u64,
    pub(in crate::chunk) words: Arc<[[u32; 2]]>,
    /// Class the uploaded order was sorted for; `None` for the mesher's unsorted order.
    pub(in crate::chunk) class: Option<FaceOrderClass>,
}

/// Past this many listed changes the next refresh rechecks every resident instead.
const MAX_LISTED_RESIDENT_CHANGES: usize = 4096;

/// Whether `allocation` carries both streams a transparent model order needs.
pub(in crate::chunk) const fn has_transparent_model_streams(
    allocation: &GpuChunkAllocation,
) -> bool {
    allocation.model_range.is_some() && allocation.transparent_model_draw_range.is_some()
}

/// Resident allocations with transparent models that were uploaded, replaced or removed
/// since the model draw orders last refreshed, so a refresh never scans the whole arena.
#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentModelResidentChanges {
    entities: Vec<Entity>,
    /// Every resident may have changed, as when a whole arena replaces another.
    all: bool,
}

impl TransparentModelResidentChanges {
    /// Changes in which every resident may have changed.
    pub(in crate::chunk) fn all_changed() -> Self {
        Self {
            entities: Vec::new(),
            all: true,
        }
    }

    /// Lists `entity` when its resident allocation goes from `previous` to `next` and either
    /// carries transparent models.
    pub(in crate::chunk) fn record(
        &mut self,
        entity: Entity,
        previous: Option<&GpuChunkAllocation>,
        next: Option<&GpuChunkAllocation>,
    ) {
        if self.all
            || !previous
                .into_iter()
                .chain(next)
                .any(has_transparent_model_streams)
        {
            return;
        }
        if self.entities.len() >= MAX_LISTED_RESIDENT_CHANGES {
            self.record_all();
        } else {
            self.entities.push(entity);
        }
    }

    /// Marks every resident as changed.
    pub(in crate::chunk) fn record_all(&mut self) {
        self.all = true;
        self.entities.clear();
    }

    /// Hands the recorded changes to a refresh, leaving none recorded.
    pub(in crate::chunk) fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentModelDrawOrders {
    entries: HashMap<Entity, TransparentModelDrawOrder>,
    next_revision: u64,
    retained_refs: usize,
    // A successful GPU sort with no room for its CPU witness must never be
    // mistaken for a newly uploaded, naturally ordered allocation next frame.
    // Its class is still kept, so a camera move re-sorts it like any witnessed order.
    unwitnessed: HashMap<Entity, (TransparentModelAllocationIdentity, Option<FaceOrderClass>)>,
    /// Entities the next refresh checks against their residents besides the listed changes:
    /// published orders and residents whose mesher order could not be recorded yet.
    pending: HashSet<Entity>,
    /// Every witness has been checked against the resident allocations once; until then a
    /// refresh scans them all.
    synced: bool,
}

/// Whether `resident` still holds the allocation `identity` names.
fn resident_matches(
    resident: Option<&GpuChunkAllocation>,
    identity: &TransparentModelAllocationIdentity,
) -> bool {
    resident.is_some_and(|gpu| {
        gpu.generation == identity.generation
            && gpu.key == identity.key
            && gpu.model_range.as_ref() == Some(&identity.model_range)
            && gpu.transparent_model_draw_range.as_ref() == Some(&identity.draw_range)
    })
}

impl TransparentModelDrawOrders {
    /// Requires a new sort class after mesh or asset inputs change, preserving order witnesses.
    pub(super) fn invalidate_classes(&mut self, entities: &HashSet<Entity>, all: bool) {
        if all {
            for order in self.entries.values_mut() {
                order.class = None;
            }
            for (_, class) in self.unwitnessed.values_mut() {
                *class = None;
            }
        } else {
            for entity in entities {
                if let Some(order) = self.entries.get_mut(entity) {
                    order.class = None;
                }
                if let Some((_, class)) = self.unwitnessed.get_mut(entity) {
                    *class = None;
                }
            }
        }
    }

    pub(in crate::chunk) fn remap_entities(&mut self, residents: &HashMap<SubChunkKey, Entity>) {
        self.entries = self
            .entries
            .drain()
            .filter_map(|(_, mut order)| {
                let &entity = residents.get(&order.identity.key)?;
                order.identity.entity = entity;
                Some((entity, order))
            })
            .collect();
        self.unwitnessed = self
            .unwitnessed
            .drain()
            .filter_map(|(_, (mut identity, class))| {
                let &entity = residents.get(&identity.key)?;
                identity.entity = entity;
                Some((entity, (identity, class)))
            })
            .collect();
        self.retained_refs = self.entries.values().map(|order| order.words.len()).sum();
        // The witnesses now name other entities, so the next refresh checks them all.
        self.synced = false;
    }

    pub(in crate::chunk) fn get(
        &self,
        identity: &TransparentModelAllocationIdentity,
    ) -> Option<&TransparentModelDrawOrder> {
        self.entries
            .get(&identity.entity)
            .filter(|entry| &entry.identity == identity)
    }

    /// Class the uploaded order of `identity` was sorted for, witnessed or not.
    pub(in crate::chunk) fn class(
        &self,
        identity: &TransparentModelAllocationIdentity,
    ) -> Option<FaceOrderClass> {
        self.get(identity).map_or_else(
            || {
                self.unwitnessed
                    .get(&identity.entity)
                    .filter(|(unwitnessed, _)| unwitnessed == identity)
                    .and_then(|(_, class)| *class)
            },
            |order| order.class,
        )
    }

    fn insert(
        &mut self,
        identity: TransparentModelAllocationIdentity,
        words: Arc<[[u32; 2]]>,
        class: Option<FaceOrderClass>,
    ) {
        let replaced = self
            .entries
            .get(&identity.entity)
            .map_or(0, |entry| entry.words.len());
        let retained = self
            .retained_refs
            .saturating_sub(replaced)
            .saturating_add(words.len());
        if retained > MAX_TRANSPARENT_DRAW_REFS {
            // Never leave a stale witness after a successful GPU write.
            self.entries.remove(&identity.entity);
            self.unwitnessed.insert(identity.entity, (identity, class));
            self.retained_refs = self.retained_refs.saturating_sub(replaced);
            return;
        }
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        self.unwitnessed.remove(&identity.entity);
        self.entries.insert(
            identity.entity,
            TransparentModelDrawOrder {
                identity,
                revision: self.next_revision,
                words,
                class,
            },
        );
        self.retained_refs = retained;
    }

    pub(in crate::chunk) fn publish(
        &mut self,
        address: &TransparentModelAddressIdentity,
        batch: TransparentModelSortBatch,
    ) {
        let Some(identity) = address
            .allocations
            .iter()
            .find(|identity| identity.draw_range == batch.draw_range)
        else {
            return;
        };
        if batch.words.len().checked_mul(2)
            != batch
                .draw_range
                .end
                .checked_sub(batch.draw_range.start)
                .map(|count| count as usize)
        {
            return;
        }
        self.insert(identity.clone(), Arc::from(batch.words), Some(batch.class));
        // A refresh drops the witness again if its allocation is no longer resident.
        self.pending.insert(identity.entity);
    }

    /// Records `batch`'s class without a new revision when its order is already uploaded.
    pub(in crate::chunk) fn reclassify_unchanged(
        &mut self,
        address: &TransparentModelAddressIdentity,
        batch: &TransparentModelSortBatch,
    ) -> bool {
        let Some(identity) = address
            .allocations
            .iter()
            .find(|identity| identity.draw_range == batch.draw_range)
        else {
            return false;
        };
        let Some(entry) = self
            .entries
            .get_mut(&identity.entity)
            .filter(|entry| &entry.identity == identity && *entry.words == *batch.words)
        else {
            return false;
        };
        entry.class = Some(batch.class);
        true
    }

    /// Brings the witnesses up to date with `residents` after `changes`, as checking every
    /// resident would: drops each witness whose allocation was replaced or removed, then
    /// records the mesher's order of each newly resident transparent model allocation.
    pub(in crate::chunk) fn refresh<'a>(
        &mut self,
        residents: &HashMap<Entity, ArenaAllocation>,
        changes: TransparentModelResidentChanges,
        instance: impl Fn(Entity) -> Option<&'a ChunkRenderInstance>,
    ) {
        if changes.all || !self.synced {
            self.refresh_all(residents, &instance);
            return;
        }
        let mut checked = std::mem::take(&mut self.pending);
        checked.extend(changes.entities);
        if checked.is_empty() {
            self.pending = checked;
            return;
        }
        let resident = |entity: &Entity| residents.get(entity).map(|resident| &resident.gpu);
        // Every stale witness goes before any order is recorded, so the ref ceiling sees it gone.
        for entity in &checked {
            if self
                .unwitnessed
                .get(entity)
                .is_some_and(|(identity, _)| !resident_matches(resident(entity), identity))
            {
                self.unwitnessed.remove(entity);
            }
            if let Some(order) = self.entries.get(entity)
                && !resident_matches(resident(entity), &order.identity)
            {
                self.retained_refs = self.retained_refs.saturating_sub(order.words.len());
                self.entries.remove(entity);
            }
        }
        checked.retain(|&entity| {
            resident(&entity).is_some_and(|gpu| !self.record_mesher_order(entity, gpu, &instance))
        });
        self.pending = checked;
    }

    /// Checks every witness and every resident, as the first refresh and a replaced arena need.
    fn refresh_all<'a>(
        &mut self,
        residents: &HashMap<Entity, ArenaAllocation>,
        instance: &impl Fn(Entity) -> Option<&'a ChunkRenderInstance>,
    ) {
        let resident = |entity: &Entity| residents.get(entity).map(|resident| &resident.gpu);
        self.unwitnessed
            .retain(|entity, (identity, _)| resident_matches(resident(entity), identity));
        self.entries
            .retain(|entity, order| resident_matches(resident(entity), &order.identity));
        self.retained_refs = self
            .entries
            .values()
            .map(|entry| entry.words.len())
            .sum::<usize>();
        self.pending.clear();
        for (&entity, resident) in residents {
            if !self.record_mesher_order(entity, &resident.gpu, instance) {
                self.pending.insert(entity);
            }
        }
        self.synced = true;
    }

    /// Records the mesher's order of `gpu`'s transparent models unless `entity` already has
    /// an order on record. Returns false while such an order is due but cannot be recorded.
    fn record_mesher_order<'a>(
        &mut self,
        entity: Entity,
        gpu: &GpuChunkAllocation,
        instance: &impl Fn(Entity) -> Option<&'a ChunkRenderInstance>,
    ) -> bool {
        // Most residents have no transparent models; reject them before any map lookup.
        let (Some(model_range), Some(draw_range)) = (
            gpu.model_range.as_ref(),
            gpu.transparent_model_draw_range.as_ref(),
        ) else {
            return true;
        };
        if self.entries.contains_key(&entity) || self.unwitnessed.contains_key(&entity) {
            return true;
        }
        let Some(instance) = instance(entity) else {
            return false;
        };
        if !transparent_model_allocation_matches(instance, gpu)
            || !model_range.start.is_multiple_of(4)
            || !draw_range.start.is_multiple_of(2)
            || instance.transparent_model_draw_refs.len().checked_mul(2)
                != Some(draw_range.end.saturating_sub(draw_range.start) as usize)
        {
            return false;
        }
        if self
            .retained_refs
            .saturating_add(instance.transparent_model_draw_refs.len())
            > MAX_TRANSPARENT_DRAW_REFS
        {
            return false;
        }
        let Some(words) = instance
            .transparent_model_draw_refs
            .iter()
            .map(|draw| {
                let [record, quad] = draw.words();
                Some([record.checked_add(model_range.start / 4)?, quad])
            })
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        self.insert(
            TransparentModelAllocationIdentity {
                entity,
                key: gpu.key,
                generation: gpu.generation,
                model_range: model_range.clone(),
                draw_range: draw_range.clone(),
            },
            words.into(),
            None,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> TransparentModelAllocationIdentity {
        TransparentModelAllocationIdentity {
            entity: Entity::from_bits(1),
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 7,
            model_range: 0..4,
            draw_range: 4..8,
        }
    }

    #[test]
    fn upload_publishes_its_exact_order_before_other_chunk_batches_complete() {
        let identity = identity();
        let address = TransparentModelAddressIdentity {
            revision: 0,
            _retained_assets: None,
            asset_identity: ChunkTextureAssetIdentity::new(0, 1),
            allocations: Arc::from([identity.clone()]),
        };
        let mut orders = TransparentModelDrawOrders::default();
        orders.insert(identity.clone(), Arc::from([[0, 0], [0, 1]]), None);
        let initial_revision = orders.get(&identity).unwrap().revision;
        orders.publish(
            &address,
            TransparentModelSortBatch {
                draw_range: identity.draw_range.clone(),
                class: FaceOrderClass::Far([0, 0, 1]),
                words: vec![[0, 1], [0, 0]].into_boxed_slice(),
            },
        );
        let uploaded = orders.get(&identity).unwrap();
        assert_eq!(uploaded.words.as_ref(), [[0, 1], [0, 0]]);
        assert_ne!(uploaded.revision, initial_revision);
        let mut replacement = identity;
        replacement.generation += 1;
        assert!(orders.get(&replacement).is_none());
    }

    #[test]
    fn witness_ceiling_keeps_successfully_sorted_but_unwitnessed_identity() {
        let identity = identity();
        let mut orders = TransparentModelDrawOrders {
            // Exercise the hard counter boundary without allocating its full buffer.
            retained_refs: MAX_TRANSPARENT_DRAW_REFS,
            ..default()
        };
        orders.insert(identity.clone(), Arc::from([[0, 1], [0, 0]]), None);
        assert!(orders.get(&identity).is_none());
        assert_eq!(
            orders.unwitnessed.get(&identity.entity),
            Some(&(identity.clone(), None))
        );
        assert_eq!(orders.retained_refs, MAX_TRANSPARENT_DRAW_REFS);
    }

    #[test]
    fn candidate_publication_remaps_both_sorted_and_unwitnessed_entities_by_world_key() {
        let temporary = identity();
        let mut unknown = temporary.clone();
        unknown.entity = Entity::from_bits(2);
        unknown.key.x += 1;
        let mut orders = TransparentModelDrawOrders::default();
        orders.insert(temporary.clone(), Arc::from([[0, 1], [0, 0]]), None);
        orders
            .unwitnessed
            .insert(unknown.entity, (unknown.clone(), None));
        let resident = Entity::from_bits(20);
        let unknown_resident = Entity::from_bits(21);
        let revision = orders.get(&temporary).unwrap().revision;
        orders.remap_entities(&HashMap::from([
            (temporary.key, resident),
            (unknown.key, unknown_resident),
        ]));
        assert!(orders.get(&temporary).is_none());
        let mut published = temporary;
        published.entity = resident;
        let witness = orders.get(&published).unwrap();
        assert_eq!(witness.words.as_ref(), [[0, 1], [0, 0]]);
        assert_eq!(witness.revision, revision);
        assert_eq!(orders.retained_refs, witness.words.len());
        unknown.entity = unknown_resident;
        assert_eq!(
            orders.unwitnessed.get(&unknown_resident),
            Some(&(unknown, None))
        );
    }

    /// A resident allocation at `slot`, with `refs` transparent model faces when `models`.
    fn resident(slot: u32, generation: u64, models: bool, refs: u32) -> ArenaAllocation {
        let base = slot * 64;
        let model_range = models.then_some(base..base + 4);
        let draw_range = models.then_some(base + 4..base + 4 + 2 * refs);
        let gpu = GpuChunkAllocation {
            cube_layout: CubeQuadLayout::default(),
            key: SubChunkKey::new(0, slot as i32, 0, 0),
            generation,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: model_range.clone(),
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: draw_range.clone(),
            liquid_range: None,
            liquid_lighting_range: None,
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_range: None,
            order_independent_liquid: false,
            metadata_index: slot,
        };
        ArenaAllocation {
            generation,
            tint_identity: gpu.tint_identity,
            cube_range: None,
            cube_lighting_range: None,
            model_range,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: draw_range,
            liquid_range: None,
            liquid_lighting_range: None,
            quad_capacity: 0,
            geometry_stream_range: Some(base..base + 64),
            geometry_stream_capacity: 64,
            biome_range: 0..0,
            biome_capacity: 0,
            biome_record: None,
            gpu,
        }
    }

    /// The mesh `resident` was uploaded from, or a stale one when `stale`.
    fn mesh(resident: &ArenaAllocation, stale: bool) -> ChunkRenderInstance {
        let gpu = &resident.gpu;
        let refs = gpu
            .transparent_model_draw_range
            .as_ref()
            .map_or(0, |range| (range.end - range.start) / 2);
        ChunkRenderInstance {
            key: gpu.key,
            origin: chunk_origin(gpu.key),
            generation: gpu.generation + u64::from(stale),
            cube_layout: CubeQuadLayout::default(),
            cube_quads: Arc::from([]),
            cube_lighting: Arc::from([]),
            model_refs: if gpu.model_range.is_some() {
                Arc::from([PackedModelRef::new(0, 0, 0, 0b111)])
            } else {
                Arc::from([])
            },
            model_lighting: Arc::from([]),
            model_draw_refs: Arc::from([]),
            transparent_model_draw_refs: (0..refs)
                .map(|quad| PackedModelDrawRef::new(0, quad))
                .collect(),
            liquid_quads: Arc::from([]),
            liquid_lighting: Arc::from([]),
            has_depth_liquid: false,
            has_transparent_liquid: false,
            depth_liquid_start: None,
            biome: meshing::PackedBiomeRecord::fallback(),
            tint_identity: ChunkBiomeTintIdentity::default(),
            priority: render_scene::ChunkUploadPriority::new(0.0),
            token: None,
            publication_permit: None,
        }
    }

    /// Everything a witness exposes except its opaque revision number.
    type Witnesses = (
        BTreeMap<
            Entity,
            (
                TransparentModelAllocationIdentity,
                Vec<[u32; 2]>,
                Option<FaceOrderClass>,
            ),
        >,
        BTreeMap<Entity, (TransparentModelAllocationIdentity, Option<FaceOrderClass>)>,
        usize,
    );

    fn witnesses(orders: &TransparentModelDrawOrders) -> Witnesses {
        (
            orders
                .entries
                .iter()
                .map(|(&entity, order)| {
                    (
                        entity,
                        (order.identity.clone(), order.words.to_vec(), order.class),
                    )
                })
                .collect(),
            orders
                .unwitnessed
                .iter()
                .map(|(&entity, witness)| (entity, witness.clone()))
                .collect(),
            orders.retained_refs,
        )
    }

    fn revisions(orders: &TransparentModelDrawOrders) -> BTreeMap<Entity, u64> {
        orders
            .entries
            .iter()
            .map(|(&entity, order)| (entity, order.revision))
            .collect()
    }

    /// Entities whose witness revision appeared, vanished or changed between two snapshots.
    fn rewitnessed(
        before: &BTreeMap<Entity, u64>,
        after: &BTreeMap<Entity, u64>,
    ) -> BTreeSet<Entity> {
        before
            .keys()
            .chain(after.keys())
            .filter(|entity| before.get(entity) != after.get(entity))
            .copied()
            .collect()
    }

    #[test]
    fn listed_changes_refresh_exactly_like_checking_every_resident() {
        const ENTITIES: u32 = 10;
        for seed in [0x1234_5678_u32, 0x9e37_79b9, 0x0bad_cafe] {
            let mut state = seed;
            let mut next = move |bound: u32| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state % bound
            };
            let entity = |slot: u32| Entity::from_bits(u64::from(slot) + 1);
            let mut residents = HashMap::<Entity, ArenaAllocation>::new();
            let mut meshes = HashMap::<Entity, ChunkRenderInstance>::new();
            let mut changes = TransparentModelResidentChanges::default();
            let mut incremental = TransparentModelDrawOrders::default();
            let mut full = TransparentModelDrawOrders::default();
            for step in 0..2_000 {
                for _ in 0..next(4) {
                    let slot = next(ENTITIES);
                    let target = entity(slot);
                    match next(5) {
                        0 | 1 => {
                            let next_resident =
                                resident(slot, u64::from(next(1_000)), next(3) != 0, 1 + next(3));
                            changes.record(
                                target,
                                residents.get(&target).map(|resident| &resident.gpu),
                                Some(&next_resident.gpu),
                            );
                            // Some meshes are replaced again before their upload lands.
                            meshes.insert(target, mesh(&next_resident, next(4) == 0));
                            residents.insert(target, next_resident);
                        }
                        2 => {
                            changes.record(
                                target,
                                residents.get(&target).map(|resident| &resident.gpu),
                                None,
                            );
                            residents.remove(&target);
                            if next(2) == 0 {
                                meshes.remove(&target);
                            }
                        }
                        3 => {
                            if let Some(resident) = residents.get(&target) {
                                meshes.insert(target, mesh(resident, next(3) == 0));
                            }
                        }
                        _ => {
                            // Sorted orders land for the current or an outdated identity.
                            let Some(resident) = residents.get(&target) else {
                                continue;
                            };
                            let gpu = &resident.gpu;
                            let (Some(model_range), Some(draw_range)) = (
                                gpu.model_range.clone(),
                                gpu.transparent_model_draw_range.clone(),
                            ) else {
                                continue;
                            };
                            let identity = TransparentModelAllocationIdentity {
                                entity: target,
                                key: gpu.key,
                                generation: gpu.generation + u64::from(next(4) == 0),
                                model_range,
                                draw_range: draw_range.clone(),
                            };
                            let address = TransparentModelAddressIdentity {
                                revision: 0,
                                _retained_assets: None,
                                asset_identity: ChunkTextureAssetIdentity::new(0, 1),
                                allocations: Arc::from([identity]),
                            };
                            let words = (0..(draw_range.end - draw_range.start) / 2)
                                .map(|quad| [next(8), quad])
                                .collect::<Vec<_>>();
                            let class = FaceOrderClass::Far([next(3) as i8 - 1, 0, 0]);
                            for orders in [&mut incremental, &mut full] {
                                orders.publish(
                                    &address,
                                    TransparentModelSortBatch {
                                        draw_range: draw_range.clone(),
                                        class,
                                        words: words.clone().into_boxed_slice(),
                                    },
                                );
                            }
                        }
                    }
                }
                let before = (revisions(&incremental), revisions(&full));
                let mesh_of = |entity: Entity| meshes.get(&entity);
                incremental.refresh(&residents, changes.take(), mesh_of);
                full.refresh(
                    &residents,
                    TransparentModelResidentChanges::all_changed(),
                    mesh_of,
                );
                assert_eq!(
                    witnesses(&incremental),
                    witnesses(&full),
                    "seed {seed:#x} step {step}"
                );
                assert_eq!(
                    rewitnessed(&before.0, &revisions(&incremental)),
                    rewitnessed(&before.1, &revisions(&full)),
                    "seed {seed:#x} step {step}: witnesses recorded again differ"
                );
            }
        }
    }

    #[test]
    fn unchanged_residents_refresh_without_visiting_them() {
        let mut residents = HashMap::new();
        let mut meshes = HashMap::new();
        for slot in 0..4 {
            let target = Entity::from_bits(u64::from(slot) + 1);
            let next = resident(slot, 1, true, 2);
            meshes.insert(target, mesh(&next, false));
            residents.insert(target, next);
        }
        let mut orders = TransparentModelDrawOrders::default();
        let mesh_of = |entity: Entity| meshes.get(&entity);
        orders.refresh(
            &residents,
            TransparentModelResidentChanges::all_changed(),
            mesh_of,
        );
        assert_eq!(orders.entries.len(), 4);
        assert!(orders.pending.is_empty());
        // With nothing listed or pending, an allocation swapped behind the log's back stays
        // witnessed: only listed changes are checked.
        let first = Entity::from_bits(1);
        residents.insert(first, resident(0, 2, true, 2));
        orders.refresh(
            &residents,
            TransparentModelResidentChanges::default(),
            mesh_of,
        );
        assert_eq!(orders.entries[&first].identity.generation, 1);
    }
}
