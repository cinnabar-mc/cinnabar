//! Client-owned display actors share admission and rig evaluation without entering network state.
use super::*;
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Arc,
};

pub const MAX_DISPLAY_ACTORS: usize = 256;
const MAX_UNRIGGED_IDENTIFIERS: usize = MAX_DISPLAY_ACTORS;

/// One bounded lifetime namespace for a dimension's displayed entities.
pub struct DisplayActorStore {
    store: ActorStore,
    positions: BTreeMap<[i32; 3], u64>,
    unrigged: HashSet<Arc<str>>, // identifiers without a rig in the current catalogs
    unrigged_order: VecDeque<Arc<str>>, // oldest first; bounds `unrigged`
    sequence: u64,
    next_runtime: u64,
}

impl DisplayActorStore {
    /// Creates the normal actor evaluator with bounded display-only storage.
    pub fn new(dimension: i32, assets: impl Into<actor_animation::ActorAnimationCatalog>) -> Self {
        let session = crate::authority::allocate_actor_session_id();
        let store = ActorStore::with_limits_and_animation(
            session,
            dimension,
            MAX_DISPLAY_ACTORS,
            0,
            MAX_TRACKED_PLAYER_SKIN_BYTES,
            actor_animation::ActorAnimationStore::with_assets(assets),
            crate::item::ItemStateStore::empty(),
        );
        Self::with_store(store)
    }

    /// Starts display bookkeeping for an independent actor namespace.
    fn with_store(store: ActorStore) -> Self {
        Self {
            store,
            positions: BTreeMap::new(),
            unrigged: HashSet::new(),
            unrigged_order: VecDeque::new(),
            sequence: 0,
            next_runtime: 0,
        }
    }

    /// Retires displays and pack bindings while retaining base catalog analysis and item rules.
    pub fn restart(self, dimension: i32) -> Self {
        Self::with_store(
            self.store
                .restart_display(crate::authority::allocate_actor_session_id(), dimension),
        )
    }

    /// Rebinds the same displayed lifetimes when the active pack changes.
    pub fn set_pack(&mut self, pack: Option<(Arc<assets::RuntimeEntityAssets>, Vec<u32>)>) {
        self.store.set_pack_entities(pack);
        self.unrigged.clear();
        self.unrigged_order.clear();
    }

    /// Lifetime namespace of every display actor this store publishes.
    pub fn session_id(&self) -> u64 {
        self.store.session_id
    }

    /// Remembers a rigless identifier, evicting the oldest once the cache is full.
    fn remember_unrigged(&mut self, identifier: &str) {
        if self.unrigged_order.len() >= MAX_UNRIGGED_IDENTIFIERS
            && let Some(oldest) = self.unrigged_order.pop_front()
        {
            self.unrigged.remove(&oldest);
        }
        let identifier: Arc<str> = Arc::from(identifier);
        self.unrigged.insert(Arc::clone(&identifier));
        self.unrigged_order.push_back(identifier);
    }

    #[cfg(test)]
    pub(crate) fn caches_unrigged(&self, identifier: &str) -> Option<usize> {
        self.unrigged
            .contains(identifier)
            .then_some(self.unrigged_order.len())
    }

    /// Reports whether `identifier` can name a display actor; callers skip others before admission.
    pub fn admits_identifier(identifier: &str) -> bool {
        !identifier.is_empty()
            && identifier.len() <= protocol::MAX_ACTOR_IDENTIFIER_BYTES
            && !identifier.chars().any(char::is_control)
    }

    /// Reuses an unchanged display actor, replacing it when its configured entity changes.
    /// Returns `None` without retaining a lifetime when the current catalogs give it no rig.
    pub fn resolve(&mut self, position: [i32; 3], identifier: &str) -> Option<u64> {
        if !Self::admits_identifier(identifier) || self.unrigged.contains(identifier) {
            self.remove(position);
            return None;
        }
        if let Some(&runtime) = self.positions.get(&position) {
            let unchanged = self.store.get(runtime).is_some_and(|actor| {
                matches!(&actor.kind,
                ActorKind::Entity { identifier: current } if current.as_ref() == identifier)
            });
            // A pack change can drop the rig of a lifetime that is otherwise unchanged.
            if unchanged && self.store.actor_rig(runtime).is_some() {
                return Some(runtime);
            }
            self.remove(position);
            if unchanged {
                self.remember_unrigged(identifier);
                return None;
            }
        }
        if self.positions.len() >= MAX_DISPLAY_ACTORS {
            return None;
        }
        self.next_runtime = self.next_runtime.checked_add(1)?;
        self.sequence = self.sequence.checked_add(1)?;
        let runtime_id = self.next_runtime;
        let event = ActorSpawnEvent {
            dimension: self.store.dimension,
            unique_id: i64::try_from(runtime_id).ok()?,
            runtime_id,
            kind: ActorKind::Entity {
                identifier: Arc::from(identifier),
            },
            position: position.map(|axis| axis as f32),
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::default(),
            attributes: Arc::default(),
            properties: Arc::default(),
            links: Arc::default(),
        };
        let result = self.store.apply(
            self.store.session_id,
            self.sequence,
            ActorEvent::Spawn(event),
        );
        if result != ActorApplyResult::Inserted {
            return None;
        }
        self.positions.insert(position, runtime_id);
        if self.store.actor_rig(runtime_id).is_none() {
            self.remove(position);
            self.remember_unrigged(identifier);
            return None;
        }
        Some(runtime_id)
    }

    /// Removes only the client-owned lifetime for this block position.
    fn remove(&mut self, position: [i32; 3]) {
        if let Some(runtime) = self.positions.remove(&position) {
            let Some(sequence) = self.sequence.checked_add(1) else {
                return;
            };
            self.sequence = sequence;
            self.store.apply(
                self.store.session_id,
                sequence,
                ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: self.store.dimension,
                    unique_id: runtime as i64,
                }),
            );
        }
    }

    /// Retires rejected displays, testing `keep` once per stored position in position order.
    pub fn retain(&mut self, mut keep: impl FnMut([i32; 3]) -> bool) {
        let removed = self
            .positions
            .keys()
            .copied()
            .filter(|position| !keep(*position))
            .collect::<Vec<_>>();
        for position in removed {
            self.remove(position);
        }
    }

    /// Feeds the resolved camera's degrees and world position before evaluating display poses.
    pub fn set_camera(&mut self, rotation: [f32; 2], position: [f32; 3]) {
        self.store.set_camera_rotation(rotation);
        self.store.set_camera_position(position);
    }

    /// Advances ordinary actor visual state once for the elapsed simulation ticks.
    pub fn advance(&mut self, ticks: u32) {
        self.store.advance_interpolation_frame(ticks);
    }

    /// Borrows the displayed snapshot for the normal presentation adapter.
    pub fn actor(&self, runtime: u64) -> Option<&ActorSnapshot> {
        self.store.get(runtime)
    }

    /// Borrows the normal rig, including its admitted render controllers.
    pub fn rig(&self, runtime: u64) -> Option<actor_animation::ActorRigSnapshot<'_>> {
        self.store.actor_rig(runtime)
    }

    /// Gives display rendering the same bounded sampled expression context as world actors.
    pub fn frame(&self, alpha: f32) -> actor_animation::ActorRenderFrame<'_> {
        self.store.render_frame(alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Removing unused item tables must preserve the ordinary display evaluator's visible pose.
    #[test]
    fn equipment_free_display_keeps_the_same_pose_as_a_retail_item_store() {
        let assets = actor_animation::test_support::counting_random_assets();
        let catalog = actor_animation::ActorAnimationCatalog::new(assets);
        let mut display = DisplayActorStore::new(0, catalog.clone());
        let mut reference = DisplayActorStore::new(0, catalog);
        reference.store.items = crate::item::ItemStateStore::diagnostic();
        reference.store.session_id = display.store.session_id;
        let runtime = display.resolve([0; 3], "minecraft:test").unwrap();
        let expected = reference.resolve([0; 3], "minecraft:test").unwrap();
        assert_eq!(runtime, expected);
        for ticks in [0, 1, 3] {
            display.advance(ticks);
            reference.advance(ticks);
            let actual = display.rig(runtime).unwrap();
            let expected = reference.rig(runtime).unwrap();
            assert_eq!(actual.current, expected.current);
            assert_eq!(actual.previous, expected.previous);
            assert_eq!(actual.render, expected.render);
            assert_eq!(actual.scale, expected.scale);
        }
    }
}
