//! Main-world frustum culling of chunk render entities.
//!
//! Bevy's `check_visibility_cpu_culling` tests every entity against every camera each frame
//! through a parallel ECS query, then sorts each visible list. Chunk entities opt out with
//! [`NoCpuCulling`]; [`ChunkVisibilitySet`] keeps their section origins in dense blocks sorted by
//! entity, and [`publish_chunk_visibility`] writes each camera's `ChunkRenderInstance` list from
//! one pass over them. An f32 plane test with a bounded rounding error decides almost every
//! section; sections within that bound of a plane repeat Bevy's own sphere-then-box test, so each
//! list equals the one Bevy would publish, already in entity order.

use std::any::TypeId;

use bevy::{
    camera::{
        Camera,
        primitives::{Aabb, Frustum, Sphere},
        visibility::{
            NoCpuCulling, RenderLayers, VisibilitySystems, VisibleEntities,
            check_visibility_cpu_culling,
        },
    },
    math::Vec3,
    prelude::{
        App, Changed, Entity, GlobalTransform, Has, InheritedVisibility, Insert,
        IntoScheduleConfigs, Local, On, PostUpdate, Query, Remove, ResMut, Resource, With,
    },
    shape::ViewFrustum,
};
use render_scene::ChunkRenderInstance;

#[cfg(test)]
mod tests;

/// Entries per block before it splits; an insertion shifts at most one block.
const BLOCK_CAPACITY: usize = 512;
/// Planes Bevy tests chunk bounds against: every plane but the far one.
const PLANE_COUNT: usize = ViewFrustum::NEAR_PLANE_IDX + 1;
/// Entry state bit: the entity's `InheritedVisibility` is true.
const SHOWN: u8 = 1;
/// Entry state bit: the entity no longer has a `ChunkRenderInstance`.
const REMOVED: u8 = 2;
/// Plane-test classes, ordered so a class is the count of margins a section clears;
/// `UNSURE` sections repeat Bevy's test exactly.
const REJECT: u8 = 0;
const UNSURE: u8 = 1;
const ACCEPT: u8 = 2;
const _: () = assert!(REJECT == 0 && UNSURE == 1 && ACCEPT == 2);
/// Bounds the rounding of Bevy's and this module's f32 plane distances, per unit of the
/// magnitudes summed into them: together they err by under 20 unit roundoffs.
const ROUNDING_PER_MAGNITUDE: f64 = 32.0 * (f32::EPSILON as f64 / 2.0);

/// Section bounds of every chunk entity, sorted by entity for allocation-free ordered output.
#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkVisibilitySet {
    /// Non-empty blocks in entity order; every entity in a block precedes the next block's.
    blocks: Vec<Block>,
}

/// One run of entries in structure-of-arrays form, so the plane test vectorises.
#[derive(Default)]
struct Block {
    entities: Vec<Entity>,
    /// Section origin: exactly the translation Bevy's culling used for the entity.
    x: Vec<f32>,
    y: Vec<f32>,
    z: Vec<f32>,
    /// Whether the section uses the padded model bounds.
    models: Vec<bool>,
    /// [`SHOWN`] and [`REMOVED`] bits.
    state: Vec<u8>,
    removed: usize,
}

impl Block {
    fn insert(&mut self, at: usize, entity: Entity, origin: [f32; 3], models: bool, state: u8) {
        self.entities.insert(at, entity);
        self.x.insert(at, origin[0]);
        self.y.insert(at, origin[1]);
        self.z.insert(at, origin[2]);
        self.models.insert(at, models);
        self.state.insert(at, state);
    }

    /// Moves the entries from `at` on into a new block.
    fn split_off(&mut self, at: usize) -> Block {
        let mut tail = Block {
            entities: self.entities.split_off(at),
            x: self.x.split_off(at),
            y: self.y.split_off(at),
            z: self.z.split_off(at),
            models: self.models.split_off(at),
            state: self.state.split_off(at),
            removed: 0,
        };
        tail.removed = tail.state.iter().filter(|&&s| s & REMOVED != 0).count();
        self.removed -= tail.removed;
        tail
    }

    /// Drops removed entries.
    fn compact(&mut self) {
        retain_live(&mut self.entities, &self.state);
        retain_live(&mut self.x, &self.state);
        retain_live(&mut self.y, &self.state);
        retain_live(&mut self.z, &self.state);
        retain_live(&mut self.models, &self.state);
        self.state.retain(|&state| state & REMOVED == 0);
        self.removed = 0;
    }

    fn origin(&self, index: usize) -> Vec3 {
        Vec3::new(self.x[index], self.y[index], self.z[index])
    }
}

/// Keeps the values whose entry `state` is not [`REMOVED`].
fn retain_live<T>(values: &mut Vec<T>, state: &[u8]) {
    let mut states = state.iter();
    values.retain(|_| states.next().is_some_and(|&state| state & REMOVED == 0));
}

impl ChunkVisibilitySet {
    /// The block that holds or would hold `entity`, and its position there.
    fn find(&self, entity: Entity) -> (usize, Result<usize, usize>) {
        let block = self
            .blocks
            .partition_point(|block| block.entities[0] <= entity)
            .saturating_sub(1);
        let position = self
            .blocks
            .get(block)
            .map_or(Err(0), |block| block.entities.binary_search(&entity));
        (block, position)
    }

    /// Records or replaces the section at `origin`; `models` selects the padded model bounds
    /// and `shown` is the entity's `InheritedVisibility`.
    pub(in crate::chunk) fn insert(
        &mut self,
        entity: Entity,
        origin: [i32; 3],
        models: bool,
        shown: bool,
    ) {
        // The same conversion as the `GlobalTransform` Bevy culled chunks with.
        let origin = origin.map(|value| value as f32);
        let state = if shown { SHOWN } else { 0 };
        let (index, position) = self.find(entity);
        if self.blocks.is_empty() {
            self.blocks.push(Block::default());
        }
        let block = &mut self.blocks[index];
        match position {
            Ok(at) => {
                if block.state[at] & REMOVED != 0 {
                    block.removed -= 1;
                }
                [block.x[at], block.y[at], block.z[at]] = origin;
                block.models[at] = models;
                block.state[at] = state;
            }
            Err(at) => {
                block.insert(at, entity, origin, models, state);
                if block.entities.len() >= BLOCK_CAPACITY {
                    let tail = block.split_off(BLOCK_CAPACITY / 2);
                    self.blocks.insert(index + 1, tail);
                }
            }
        }
    }

    /// Forgets `entity`; its storage is reclaimed once half its block is removed.
    pub(in crate::chunk) fn remove(&mut self, entity: Entity) {
        let (index, Ok(at)) = self.find(entity) else {
            return;
        };
        let block = &mut self.blocks[index];
        if block.state[at] & REMOVED != 0 {
            return;
        }
        block.state[at] = REMOVED;
        block.removed += 1;
        if block.removed * 2 >= block.entities.len() {
            block.compact();
            if block.entities.is_empty() {
                self.blocks.remove(index);
            }
        }
    }

    /// Updates the entity's `InheritedVisibility`; unknown entities are ignored.
    pub(in crate::chunk) fn set_shown(&mut self, entity: Entity, shown: bool) {
        let (index, Ok(at)) = self.find(entity) else {
            return;
        };
        let state = &mut self.blocks[index].state[at];
        if *state & REMOVED == 0 {
            *state = if shown { SHOWN } else { 0 };
        }
    }

    /// Appends, in entity order, every shown entity, as Bevy lists them for a camera with
    /// `NoCpuCulling`.
    pub(in crate::chunk) fn shown_into(&self, out: &mut Vec<Entity>) {
        for block in &self.blocks {
            out.extend(
                block
                    .entities
                    .iter()
                    .zip(&block.state)
                    .filter(|&(_, &state)| state == SHOWN)
                    .map(|(&entity, _)| entity),
            );
        }
    }

    /// Appends, in entity order, every shown entity Bevy's CPU culling finds in `frustum`.
    pub(in crate::chunk) fn cull_into(
        &self,
        frustum: &Frustum,
        out: &mut Vec<Entity>,
        scratch: &mut CullScratch,
    ) {
        let planes = CullPlanes::new(frustum);
        let kinds = [super::bounds::aabb(false), super::bounds::aabb(true)];
        scratch.classes.resize(BLOCK_CAPACITY, REJECT);
        scratch.kept.resize(BLOCK_CAPACITY, Entity::PLACEHOLDER);
        for block in &self.blocks {
            let count = block.entities.len();
            let classes = &mut scratch.classes[..count];
            planes.classify(block, classes);
            // Every entry is written and only kept ones advance, so a random mix of visible
            // and hidden sections costs no mispredicted branch.
            let mut kept = 0;
            for (index, &class) in classes.iter().enumerate() {
                let shown = block.state[index] == SHOWN;
                let exact = class == UNSURE
                    && shown
                    && bevy_visible(
                        frustum,
                        block.origin(index),
                        &kinds[usize::from(block.models[index])],
                    );
                scratch.kept[kept] = block.entities[index];
                kept += usize::from(((class == ACCEPT) & shown) | exact);
            }
            out.extend_from_slice(&scratch.kept[..kept]);
        }
    }
}

/// Per-block buffers reused across culling passes.
#[derive(Default)]
pub(in crate::chunk) struct CullScratch {
    classes: Vec<u8>,
    kept: Vec<Entity>,
}

/// Bevy's `check_visibility_cpu_culling` frustum test for an entity with `bounds` translated to
/// `origin`, computed by the same calls so it rounds identically.
pub(in crate::chunk) fn bevy_visible(frustum: &Frustum, origin: Vec3, bounds: &Aabb) -> bool {
    let transform = GlobalTransform::from_translation(origin);
    let world_from_local = transform.affine();
    let sphere = Sphere {
        center: world_from_local.transform_point3a(bounds.center),
        radius: transform.radius_vec3a(bounds.half_extents),
    };
    frustum.intersects_sphere(&sphere, false)
        && frustum.intersects_obb(bounds, &world_from_local, true, false)
}

/// One frustum plane prepared for the f32 section test.
#[derive(Clone, Copy, Default)]
struct CullPlane {
    normal: [f32; 3],
    /// Per bounds kind: signed distance of the farthest bounds corner along the normal, for a
    /// section at the origin.
    reach: [f32; 2],
}

/// The planes Bevy tests, and the rounding margin that makes their f32 verdicts certain.
struct CullPlanes {
    planes: [CullPlane; PLANE_COUNT],
    /// The margin is `margin_scale * max(|x|, |y|, |z|) + margin_base` for a section origin.
    margin_scale: f32,
    margin_base: f32,
    /// How far Bevy's sphere test can trail its box test, for planes with non-unit normals.
    accept_extra: f32,
    /// Non-finite planes leave every section to Bevy's test.
    exact_only: bool,
}

impl CullPlanes {
    fn new(frustum: &Frustum) -> Self {
        let kinds = [super::bounds::aabb(false), super::bounds::aabb(true)];
        let mut planes = [CullPlane::default(); PLANE_COUNT];
        let mut exact_only = false;
        let (mut normal_sum, mut base, mut accept_extra) = (0.0f64, 0.0f64, 0.0f64);
        for (plane, half_space) in planes.iter_mut().zip(&frustum.half_spaces) {
            let normal_d = half_space.normal_d();
            exact_only |= !normal_d.is_finite();
            let normal = normal_d.truncate().as_dvec3();
            let d = f64::from(normal_d.w);
            normal_sum = normal_sum.max(normal.abs().element_sum());
            for (reach, bounds) in plane.reach.iter_mut().zip(&kinds) {
                let center = bounds.center.as_dvec3();
                let half = bounds.half_extents.as_dvec3();
                let radius = normal.abs().dot(half);
                *reach = (normal.dot(center) + d + radius) as f32;
                base = base.max(
                    normal.abs().element_sum() * center.abs().max_element() + d.abs() + radius,
                );
                accept_extra = accept_extra.max(radius - half.length());
            }
            plane.normal = normal_d.truncate().to_array();
        }
        Self {
            planes,
            margin_scale: (ROUNDING_PER_MAGNITUDE * normal_sum) as f32,
            margin_base: (ROUNDING_PER_MAGNITUDE * base) as f32,
            accept_extra: accept_extra as f32,
            exact_only,
        }
    }

    /// Writes [`REJECT`], [`UNSURE`] or [`ACCEPT`] for each section of `block`.
    ///
    /// The lowest plane distance decides: Bevy rejects a section whose box lies outside one
    /// plane, and accepts one whose box and sphere reach inside every plane.
    fn classify(&self, block: &Block, classes: &mut [u8]) {
        if self.exact_only {
            classes.fill(UNSURE);
            return;
        }
        let sections = block
            .x
            .iter()
            .zip(&block.y)
            .zip(&block.z)
            .zip(&block.models);
        for ((((&x, &y), &z), &models), class) in sections.zip(classes) {
            let mut lowest = f32::MAX;
            for plane in &self.planes {
                let reach = if models {
                    plane.reach[1]
                } else {
                    plane.reach[0]
                };
                let distance =
                    plane.normal[0] * x + plane.normal[1] * y + plane.normal[2] * z + reach;
                lowest = if distance < lowest { distance } else { lowest };
            }
            let magnitude = larger(larger(x.abs(), y.abs()), z.abs());
            let margin = self.margin_scale * magnitude + self.margin_base;
            *class = u8::from(lowest >= -margin) + u8::from(lowest > margin + self.accept_extra);
        }
    }
}

/// `f32::max` without its NaN handling, which keeps the plane test vectorised.
fn larger(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// Replaces Bevy's CPU culling of chunk entities, which carry `NoCpuCulling`, with
/// [`publish_chunk_visibility`].
pub(in crate::chunk) fn install(app: &mut App) {
    app.init_resource::<ChunkVisibilitySet>()
        .add_observer(track_inserted_chunk)
        .add_observer(forget_removed_chunk)
        .add_systems(
            PostUpdate,
            publish_chunk_visibility
                .in_set(VisibilitySystems::CheckVisibility)
                // Bevy's pass clears every active camera's lists first.
                .after(check_visibility_cpu_culling),
        );
}

/// Records an inserted or replaced chunk instance's section bounds.
fn track_inserted_chunk(
    insert: On<Insert<ChunkRenderInstance>>,
    chunks: Query<(&ChunkRenderInstance, Option<&InheritedVisibility>)>,
    mut set: ResMut<ChunkVisibilitySet>,
) {
    let Ok((instance, inherited)) = chunks.get(insert.entity) else {
        return;
    };
    set.insert(
        insert.entity,
        instance.origin,
        !instance.model_refs.is_empty(),
        inherited.is_some_and(|inherited| inherited.get()),
    );
}

/// Forgets a chunk entity whose instance was removed or despawned.
fn forget_removed_chunk(
    remove: On<Remove<ChunkRenderInstance>>,
    mut set: ResMut<ChunkVisibilitySet>,
) {
    set.remove(remove.entity);
}

/// Chunk entities whose `InheritedVisibility` changed since the last publication.
type ShownChanged = (With<ChunkRenderInstance>, Changed<InheritedVisibility>);

/// What Bevy's CPU culling reads from each view.
type CullView = (
    &'static mut VisibleEntities,
    &'static Frustum,
    Option<&'static RenderLayers>,
    &'static Camera,
    Has<NoCpuCulling>,
);

/// Publishes each active camera's visible chunk entities into its [`VisibleEntities`], in
/// place of Bevy's CPU culling.
fn publish_chunk_visibility(
    mut set: ResMut<ChunkVisibilitySet>,
    shown: Query<(Entity, &InheritedVisibility), ShownChanged>,
    mut views: Query<CullView>,
    mut scratch: Local<CullScratch>,
) {
    for (entity, inherited) in &shown {
        set.set_shown(entity, inherited.get());
    }
    let set = &*set;
    let class = TypeId::of::<ChunkRenderInstance>();
    for (mut visible, frustum, layers, camera, no_cpu_culling) in &mut views {
        if !camera.is_active {
            continue;
        }
        let list = visible.get_mut(class);
        list.clear();
        // Chunk entities carry no `RenderLayers`, so they sit on the default layer.
        if !layers.is_none_or(|layers| layers.intersects(&RenderLayers::default())) {
            continue;
        }
        if no_cpu_culling {
            set.shown_into(list);
        } else {
            set.cull_into(frustum, list, &mut scratch);
        }
    }
}
