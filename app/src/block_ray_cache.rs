//! Reuses block-interaction ray results while every input they read is unchanged.
//!
//! The block selection outline, the frame's block-use picks and the attack pick each cast the
//! interaction ray, often with the same ray, context and world in one frame, and an idle player
//! repeats the same casts frame after frame. A query reads only the ray, its pick context, the
//! collision store, the collision registry and the session's overlay block ids. The key holds the
//! store's content revision and collision generation, the registry's address and change tick,
//! the dimension, the overlay ids' resolution identity, the pick context and the exact bits of
//! every ray argument, so a cached result is the value the query would compute now, including
//! the column revisions it records. Piston motion is covered because a moving arm revises its
//! column, which retires the collision generation.

use std::{
    any::Any,
    sync::{Arc, Mutex},
};

use assets::block_shapes::PickContext;
use bevy::ecs::change_detection::Tick;
use sim::{BlockHit, CollisionQuery, CollisionRegistry, PaletteWorld, Vec3, WorldQueryError};
use world::{BlockIds, ChunkStore};

/// Distinct queries kept; one frame asks a handful (outline, two block-use picks, their miss
/// supports and the attack pick).
const CAPACITY: usize = 8;

type Hit = Result<Option<BlockHit>, WorldQueryError>;
type Observed = Result<CollisionQuery<Hit>, WorldQueryError>;

#[derive(Clone, Copy, PartialEq)]
enum Query {
    /// [`PaletteWorld::block_interaction_ray_current`], or its prefix up to `prefix` bits.
    Ray { reach: u64, prefix: Option<u64> },
    /// [`PaletteWorld::block_use_miss_support_current`].
    MissSupport,
    /// [`PaletteWorld::block_interaction_ray_current`] inside [`PaletteWorld::observe`].
    Observed { reach: u64 },
}

/// Every input of one query except the overlay ids, which compare through their own identity.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    content: u64,
    collisions: u64,
    registry: usize,
    registry_changed: Tick,
    dimension: i32,
    context: PickContext,
    origin: [u64; 3],
    direction: [u64; 3],
    query: Query,
}

#[derive(Clone)]
enum Value {
    Hit(Hit),
    Observed(Observed),
}

struct Entry {
    key: Key,
    overlays: Arc<dyn Any + Send + Sync>,
    value: Value,
}

/// Recent block-ray results, oldest first.
#[derive(Default)]
pub(crate) struct BlockRayCache(Mutex<Vec<Entry>>);

/// The interaction ray's palette view, with its ray queries answered through a [`BlockRayCache`].
pub(crate) struct CachedPickWorld<'a> {
    cache: &'a BlockRayCache,
    store: &'a ChunkStore,
    registry: &'a CollisionRegistry,
    registry_changed: Tick,
    dimension: i32,
    context: PickContext,
    overlays: &'a (dyn BlockIds + Sync),
}

impl<'a> CachedPickWorld<'a> {
    /// `registry_changed` is the last change tick of the resource owning `registry`, so an
    /// in-place registry edit retires every result read through it.
    pub(crate) fn new(
        cache: &'a BlockRayCache,
        stream: &'a chunk_pipeline::WorldStream,
        registry: &'a CollisionRegistry,
        registry_changed: Tick,
        context: PickContext,
        overlays: &'a (dyn BlockIds + Sync),
    ) -> Self {
        Self {
            cache,
            store: stream.collision_store(),
            registry,
            registry_changed,
            dimension: stream.current_dimension(),
            context,
            overlays,
        }
    }

    /// The uncached palette view every cached query is computed on.
    pub(crate) fn world(&self) -> PaletteWorld<'a> {
        PaletteWorld::new(self.store, self.registry, self.dimension, self.context)
            .with_cell_overlays(self.overlays)
    }

    /// [`PaletteWorld::block_interaction_ray_current`].
    pub(crate) fn ray(&self, origin: Vec3, direction: Vec3, reach: f64) -> Hit {
        let query = Query::Ray {
            reach: reach.to_bits(),
            prefix: None,
        };
        self.hit(origin, direction, query, || {
            self.world()
                .block_interaction_ray_current(origin, direction, reach)
        })
    }

    /// [`PaletteWorld::block_interaction_ray_prefix_current`]. A finite prefix at least as long
    /// as the ray searches the whole ray, so it shares [`Self::ray`]'s result.
    pub(crate) fn ray_prefix(&self, origin: Vec3, direction: Vec3, reach: f64, prefix: f64) -> Hit {
        if prefix.is_finite() && prefix >= reach {
            return self.ray(origin, direction, reach);
        }
        let query = Query::Ray {
            reach: reach.to_bits(),
            prefix: Some(prefix.to_bits()),
        };
        self.hit(origin, direction, query, || {
            self.world()
                .block_interaction_ray_prefix_current(origin, direction, reach, prefix)
        })
    }

    /// [`PaletteWorld::block_use_miss_support_current`].
    pub(crate) fn miss_support(&self, origin: Vec3, direction: Vec3) -> Hit {
        self.hit(origin, direction, Query::MissSupport, || {
            self.world()
                .block_use_miss_support_current(origin, direction)
        })
    }

    /// [`PaletteWorld::block_interaction_ray_current`] with every column it read.
    pub(crate) fn observed_ray(&self, origin: Vec3, direction: Vec3, reach: f64) -> Observed {
        let query = Query::Observed {
            reach: reach.to_bits(),
        };
        let value = self.cached(origin, direction, query, || {
            Value::Observed(
                self.world()
                    .observe(|world| world.block_interaction_ray_current(origin, direction, reach)),
            )
        });
        match value {
            Value::Observed(observed) => observed,
            Value::Hit(_) => unreachable!("observed queries store observed values"),
        }
    }

    fn hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        query: Query,
        compute: impl FnOnce() -> Hit,
    ) -> Hit {
        match self.cached(origin, direction, query, || Value::Hit(compute())) {
            Value::Hit(hit) => hit,
            Value::Observed(_) => unreachable!("ray queries store hits"),
        }
    }

    /// Returns the retained value for identical inputs, otherwise computes and retains it.
    /// Overlay ids without a stable identity are never retained.
    fn cached(
        &self,
        origin: Vec3,
        direction: Vec3,
        query: Query,
        compute: impl FnOnce() -> Value,
    ) -> Value {
        let bits = |value: Vec3| [value.x.to_bits(), value.y.to_bits(), value.z.to_bits()];
        let key = Key {
            content: self.store.content_revision(),
            collisions: self.store.collision_generation(),
            registry: std::ptr::from_ref(self.registry) as usize,
            registry_changed: self.registry_changed,
            dimension: self.dimension,
            context: self.context,
            origin: bits(origin),
            direction: bits(direction),
            query,
        };
        if let Some(entry) = self.lock().iter().find(|entry| {
            entry.key == key && self.overlays.inline_cache_context_matches(&*entry.overlays)
        }) {
            return entry.value.clone();
        }
        let value = compute();
        if let Some(overlays) = self.overlays.inline_cache_context() {
            let mut entries = self.lock();
            if entries.len() == CAPACITY {
                entries.remove(0);
            }
            entries.push(Entry {
                key,
                overlays,
                value: value.clone(),
            });
        }
        value
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.cache.0.lock().expect("block ray cache lock")
    }
}

#[cfg(test)]
#[path = "block_ray_cache/tests.rs"]
mod tests;
