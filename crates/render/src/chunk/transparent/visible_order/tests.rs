use super::*;
use bevy::{
    core_pipeline::Core3d,
    ecs::schedule::{Schedule, ScheduleLabel},
    render::camera::ExtractedCamera,
};
use render_scene::ChunkUploadPriority;

/// Builds a visible water fixture, alternating its order dependence by key.
fn water(x: i32, entity: u64) -> VisibleWater {
    VisibleWater {
        key: SubChunkKey::new(0, x, 0, 0),
        entity: Entity::from_bits(entity),
        main: MainEntity::from(Entity::from_bits(entity)),
        order_independent: x % 2 == 0,
    }
}

#[test]
fn duplicate_keys_draw_the_indexed_resident() {
    let key = SubChunkKey::new(0, 1, 0, 0);
    let (older, newer) = (water(1, 1), water(1, 7));
    let mut residents = TransparentLiquidResidents::default();
    let mut upload = allocation(key, 2, Content::Water, tint(0));
    upload.order_independent_liquid = true;
    residents.record(newer.entity, &upload);
    let ordered = [older, newer, water(3, 3)];
    assert_eq!(
        each_visible_key(&ordered, &residents).collect::<Vec<_>>(),
        [newer, water(3, 3)]
    );
}

/// Tint tables the randomized frames switch between.
fn tint(revision: u64) -> ChunkBiomeTintIdentity {
    ChunkBiomeTintIdentity::new(0, revision)
}

/// What a fixture allocation carries.
#[derive(Clone, Copy, Debug)]
enum Content {
    Cubes,
    /// Both model streams with a valid direct draw.
    Models,
    /// Both model streams, but no lighting stream, so preparation sees it and nothing draws it.
    UndrawnModels,
    Water,
    ModelsAndWater,
}

const CONTENTS: [Content; 5] = [
    Content::Cubes,
    Content::Models,
    Content::UndrawnModels,
    Content::Water,
    Content::ModelsAndWater,
];

fn allocation(
    key: SubChunkKey,
    generation: u64,
    content: Content,
    tint_identity: ChunkBiomeTintIdentity,
) -> GpuChunkAllocation {
    let models = matches!(
        content,
        Content::Models | Content::UndrawnModels | Content::ModelsAndWater
    );
    let water = matches!(content, Content::Water | Content::ModelsAndWater);
    GpuChunkAllocation {
        cube_layout: CubeQuadLayout::default(),
        key,
        generation,
        tint_identity,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: models.then_some(0..4),
        model_lighting_range: matches!(content, Content::Models | Content::ModelsAndWater)
            .then_some(4..6),
        model_draw_range: None,
        transparent_model_draw_range: models.then_some(6..8),
        liquid_range: water.then_some(8..24),
        liquid_lighting_range: water.then_some(24..32),
        has_depth_liquid: false,
        has_transparent_liquid: water,
        depth_liquid_range: None,
        order_independent_liquid: generation.is_multiple_of(2),
        metadata_index: 1,
    }
}

fn instance(key: SubChunkKey, generation: u64) -> ChunkRenderInstance {
    ChunkRenderInstance {
        key,
        origin: chunk_origin(key),
        generation,
        cube_layout: CubeQuadLayout::default(),
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: meshing::PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
    }
}

fn extracted_camera() -> ExtractedCamera {
    ExtractedCamera {
        target: None,
        physical_viewport_size: None,
        physical_target_size: None,
        viewport: None,
        schedule: Core3d.intern(),
        order: 0,
        output_mode: default(),
        msaa_writeback: default(),
        clear_color: default(),
        sorted_camera_index_for_target: 0,
        exposure: 1.0,
        hdr: false,
    }
}

/// The lists the queue and preparation built by scanning every visible entity each frame.
#[derive(Debug, PartialEq, Eq)]
struct Scanned {
    sorted_models: Vec<Entity>,
    models: Vec<VisibleModel>,
    water: Vec<VisibleWater>,
}

/// A render world whose chunk allocations, meshes, visibility and tint table change at random,
/// tracked by the production system and observers.
struct Harness {
    world: World,
    schedule: Schedule,
    view: Entity,
    /// Live render entities with the main entity Bevy lists them by.
    chunks: Vec<(Entity, MainEntity)>,
    next_main: u64,
    /// A render entity was replaced this frame, so its extraction must list the new pair.
    resynced: bool,
    seed: u32,
}

impl Harness {
    fn new(seed: u32) -> Self {
        let mut world = World::new();
        install_visible_transparent_tracking(&mut world);
        world.init_resource::<TransparentSortRuntime>();
        world.insert_resource(ChunkBiomeTints::with_identity(Arc::from([]), tint(0)));
        let view = world.spawn(extracted_camera()).id();
        world.resource_mut::<TransparentSortRuntime>().view_entity = Some(view);
        let mut schedule = Schedule::default();
        schedule.add_systems(track_visible_transparent_chunks);
        Self {
            world,
            schedule,
            view,
            chunks: Vec::new(),
            next_main: 1_000,
            resynced: false,
            seed,
        }
    }

    fn next(&mut self, bound: u32) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed % bound
    }

    fn random_allocation(&mut self) -> GpuChunkAllocation {
        let key = SubChunkKey::new(0, self.next(6) as i32, 0, self.next(2) as i32);
        let content = CONTENTS[self.next(CONTENTS.len() as u32) as usize];
        let generation = u64::from(self.next(4));
        let tint = tint(u64::from(self.next(2)));
        allocation(key, generation, content, tint)
    }

    /// Spawns a chunk under a fresh main entity, numbered against render entity order.
    fn spawn(&mut self) {
        let main = MainEntity::from(Entity::from_bits(self.next_main));
        self.next_main -= 1;
        let allocation = self.random_allocation();
        let entity = self
            .world
            .spawn((
                main,
                instance(allocation.key, allocation.generation),
                allocation,
            ))
            .id();
        self.chunks.push((entity, main));
    }

    /// Replaces a main entity's render entity, as syncing a respawned main entity would; that
    /// frame's extraction lists the new pair, which Bevy's diff by main entity does not report.
    fn resync(&mut self, index: usize) {
        let (old, main) = self.chunks[index];
        self.world.despawn(old);
        let allocation = self.random_allocation();
        let entity = self
            .world
            .spawn((
                main,
                instance(allocation.key, allocation.generation),
                allocation,
            ))
            .id();
        self.chunks[index] = (entity, main);
        self.resynced = true;
    }

    /// Changes allocations, meshes and liveness, returning the render entities whose
    /// allocation or mesh changed.
    fn mutate(&mut self) -> Vec<Entity> {
        let mut touched = Vec::new();
        for _ in 0..self.next(4) {
            if self.chunks.is_empty() || self.next(5) == 0 {
                self.spawn();
                continue;
            }
            let index = self.next(self.chunks.len() as u32) as usize;
            let entity = self.chunks[index].0;
            // Removing an absent component changes nothing.
            let has_allocation = self.world.get::<GpuChunkAllocation>(entity).is_some();
            let has_mesh = self.world.get::<ChunkRenderInstance>(entity).is_some();
            let kind = self.next(6);
            if (kind != 1 || has_allocation) && (kind != 3 || has_mesh) {
                touched.push(entity);
            }
            match kind {
                0 => {
                    let allocation = self.random_allocation();
                    self.world.entity_mut(entity).insert(allocation);
                }
                1 => {
                    self.world.entity_mut(entity).remove::<GpuChunkAllocation>();
                }
                2 => {
                    let key = SubChunkKey::new(0, 0, 0, 0);
                    self.world.entity_mut(entity).insert(instance(key, 9));
                }
                3 => {
                    self.world
                        .entity_mut(entity)
                        .remove::<ChunkRenderInstance>();
                }
                4 => {
                    // Bevy drops a despawned entity from the visible list a frame later.
                    self.world.despawn(entity);
                    self.chunks.swap_remove(index);
                }
                _ => self.resync(index),
            }
        }
        touched
    }

    /// Lists a random subset of the chunks as visible through Bevy's own diffing update.
    fn refresh_visibility(&mut self) {
        let mut visible = Vec::new();
        for index in 0..self.chunks.len() {
            if self.next(5) < 3 {
                visible.push(self.chunks[index]);
            }
        }
        self.set_visible(visible);
    }

    fn set_visible(&mut self, mut visible: Vec<(Entity, MainEntity)>) {
        visible.sort_unstable_by_key(|(_, main)| *main);
        let mut view = self.world.entity_mut(self.view);
        let mut visibility = view.get_mut::<RenderVisibleEntities>().unwrap();
        let class = visibility
            .classes
            .entry(std::any::TypeId::of::<ChunkRenderInstance>())
            .or_default();
        class.prepare_for_new_frame();
        class.update_cpu_culled_entities(&visible);
    }

    /// Re-lists the current visible pairs under each main entity's current render entity.
    fn relist_visible(&mut self) {
        let listed = self
            .world
            .get::<RenderVisibleEntities>(self.view)
            .unwrap()
            .get::<ChunkRenderInstance>()
            .map(|class| class.entities_cpu_culling.clone())
            .unwrap_or_default();
        let visible = listed
            .into_iter()
            .filter_map(|(_, main)| self.chunks.iter().find(|chunk| chunk.1 == main).copied())
            .collect();
        self.set_visible(visible);
    }

    fn set_tint(&mut self, revision: u64) {
        self.world.resource_mut::<ChunkBiomeTints>().identity = tint(revision);
    }

    fn track(&mut self) {
        self.schedule.run(&mut self.world);
    }

    /// What scanning every visible entity, as the queue and preparation did, finds now.
    fn scan(&self) -> Scanned {
        let active = self.world.resource::<ChunkBiomeTints>().identity;
        let mut scanned = Scanned {
            sorted_models: Vec::new(),
            models: Vec::new(),
            water: Vec::new(),
        };
        let visible = self.world.get::<RenderVisibleEntities>(self.view).unwrap();
        for (&entity, &main) in visible
            .get::<ChunkRenderInstance>()
            .into_iter()
            .flat_map(|class| class.iter_visible())
        {
            let Some(allocation) = self.world.get::<GpuChunkAllocation>(entity) else {
                continue;
            };
            if allocation.model_range.is_some() && allocation.transparent_model_draw_range.is_some()
            {
                scanned.sorted_models.push(entity);
            }
            if transparent_model_direct_draw_command(allocation).is_some() {
                scanned.models.push(VisibleModel {
                    key: allocation.key,
                    entity,
                    main,
                });
            }
            if transparent_liquid_direct_draw_command(allocation).is_some()
                && chunk_tint_identity_is_active(allocation.tint_identity, active)
            {
                scanned.water.push(VisibleWater {
                    key: allocation.key,
                    entity,
                    main,
                    order_independent: allocation.order_independent_liquid,
                });
            }
        }
        scanned.models.reverse();
        scanned.models.sort_by_key(|model| model.key);
        scanned.models.dedup_by_key(|model| model.key);
        scanned
            .water
            .sort_unstable_by_key(|water| (water.key, water.entity));
        scanned
    }

    fn tracked(&self) -> Scanned {
        let chunks = self.world.resource::<VisibleTransparentChunks>();
        Scanned {
            sorted_models: chunks.sorted_models().collect(),
            models: chunks.models().to_vec(),
            water: chunks.water().to_vec(),
        }
    }

    fn model_revision(&self) -> u64 {
        self.world
            .resource::<VisibleTransparentChunks>()
            .model_revision()
    }
}

#[test]
fn tracked_lists_match_a_full_visible_scan_over_random_frames() {
    for seed in [0x1234_5678, 0x9e37_79b9, 0x0bad_cafe, 0x5eed_1e55] {
        let mut harness = Harness::new(seed);
        for _ in 0..12 {
            harness.spawn();
        }
        let mut previous: Option<(u64, Vec<Entity>)> = None;
        for frame in 0..600 {
            let touched = harness.mutate();
            // Bevy refreshes the visible list once per frame; some frames skip it to cover
            // a list the tracker already saw.
            match harness.next(6) {
                0 if !harness.resynced => {}
                0 | 1 => harness.relist_visible(),
                _ => harness.refresh_visibility(),
            }
            harness.resynced = false;
            if harness.next(16) == 0 {
                let revision = u64::from(harness.next(2));
                harness.set_tint(revision);
            }
            harness.track();
            let scanned = harness.scan();
            assert_eq!(harness.tracked(), scanned, "seed {seed:#x} frame {frame}");
            let revision = harness.model_revision();
            if let Some((last_revision, last_sorted)) = &previous
                && *last_revision == revision
            {
                assert_eq!(
                    last_sorted, &scanned.sorted_models,
                    "seed {seed:#x} frame {frame}: unchanged revision but new model set"
                );
                assert!(
                    !scanned
                        .sorted_models
                        .iter()
                        .any(|entity| touched.contains(entity)),
                    "seed {seed:#x} frame {frame}: unchanged revision over a changed model"
                );
            }
            previous = Some((revision, scanned.sorted_models));
        }
    }
}

#[test]
fn idle_frames_keep_the_model_revision() {
    let mut harness = Harness::new(0x2545_f491);
    for _ in 0..16 {
        harness.spawn();
    }
    harness.refresh_visibility();
    harness.track();
    let revision = harness.model_revision();
    let tracked = harness.tracked();
    for _ in 0..4 {
        // Bevy refreshes the visible list every frame even when nothing entered or left.
        harness.relist_visible();
        harness.track();
        assert_eq!(harness.model_revision(), revision);
        assert_eq!(harness.tracked(), tracked);
    }
}

#[test]
fn a_replaced_visible_list_without_diffs_is_rebuilt() {
    let mut harness = Harness::new(0x7f4a_7c15);
    for _ in 0..16 {
        harness.spawn();
    }
    harness.refresh_visibility();
    harness.track();
    // A camera whose visible component was recreated lists everything without a matching
    // removal of what the tracker held.
    let visible = harness.chunks[..3].to_vec();
    let mut view = harness.world.entity_mut(harness.view);
    let mut visibility = view.get_mut::<RenderVisibleEntities>().unwrap();
    *visibility = RenderVisibleEntities::default();
    harness.set_visible(visible);
    harness.track();
    assert_eq!(harness.tracked(), harness.scan());
}

#[test]
fn losing_the_view_forgets_its_entries() {
    let mut harness = Harness::new(0x1b87_3593);
    for _ in 0..8 {
        harness.spawn();
    }
    harness.refresh_visibility();
    harness.track();
    harness
        .world
        .resource_mut::<TransparentSortRuntime>()
        .view_entity = None;
    harness.track();
    let chunks = harness.world.resource::<VisibleTransparentChunks>();
    assert_eq!(chunks.view(), None);
    assert_eq!(chunks.sorted_models().count(), 0);
    assert!(chunks.models().is_empty() && chunks.water().is_empty());
}
