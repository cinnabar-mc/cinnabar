//! Live actors, stamping an actor's presentation revision whenever it may change.
use super::{ActorSnapshot, FastHashMap};
use std::collections::HashMap;

/// Actors by runtime id. Reads go through `Deref`; every mutable access gives the actor a new
/// presentation revision, so a reader holding an older revision knows the actor may differ.
/// Setters that run every frame use [`Self::update`] and stamp only real changes.
#[derive(Debug, Default)]
pub(super) struct TrackedActors {
    actors: HashMap<u64, ActorSnapshot>,
    revisions: FastHashMap<u64, u64>,
    last_revision: u64,
}

impl std::ops::Deref for TrackedActors {
    type Target = HashMap<u64, ActorSnapshot>;

    fn deref(&self) -> &Self::Target {
        &self.actors
    }
}

impl actor_animation::AnimatedActors for TrackedActors {
    fn animated_actor(&self, runtime_id: u64) -> Option<&dyn actor_animation::AnimatedActor> {
        self.actors.animated_actor(runtime_id)
    }
}

impl TrackedActors {
    /// A revision no actor has held before.
    fn next_revision(&mut self) -> u64 {
        self.last_revision = self.last_revision.wrapping_add(1).max(1);
        self.last_revision
    }

    /// The actor's current presentation revision: equal values mean the snapshot is unchanged.
    pub(super) fn revision(&self, runtime_id: u64) -> Option<u64> {
        self.revisions.get(&runtime_id).copied()
    }

    /// Borrows one actor for mutation, assuming it changes.
    pub(super) fn get_mut(&mut self, runtime_id: &u64) -> Option<&mut ActorSnapshot> {
        let actor = self.actors.get_mut(runtime_id)?;
        self.last_revision = self.last_revision.wrapping_add(1).max(1);
        self.revisions.insert(*runtime_id, self.last_revision);
        Some(actor)
    }

    /// Runs `update`, stamping the actor only when it reports a change. `update` must not
    /// change any snapshot field when it returns `false`.
    pub(super) fn update<R>(
        &mut self,
        runtime_id: u64,
        update: impl FnOnce(&mut ActorSnapshot) -> (bool, R),
    ) -> Option<R> {
        let actor = self.actors.get_mut(&runtime_id)?;
        let (changed, result) = update(actor);
        if changed {
            let revision = self.next_revision();
            self.revisions.insert(runtime_id, revision);
        }
        Some(result)
    }

    /// Borrows every actor for mutation, assuming each changes.
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut ActorSnapshot> {
        self.iter_mut().map(|(_, actor)| actor)
    }

    /// Borrows every actor for mutation, assuming each changes.
    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (&u64, &mut ActorSnapshot)> {
        let revision = self.next_revision();
        for stamp in self.revisions.values_mut() {
            *stamp = revision;
        }
        self.actors.iter_mut()
    }

    pub(super) fn insert(
        &mut self,
        runtime_id: u64,
        actor: ActorSnapshot,
    ) -> Option<ActorSnapshot> {
        let revision = self.next_revision();
        self.revisions.insert(runtime_id, revision);
        self.actors.insert(runtime_id, actor)
    }

    pub(super) fn remove(&mut self, runtime_id: &u64) -> Option<ActorSnapshot> {
        self.revisions.remove(runtime_id);
        self.actors.remove(runtime_id)
    }

    pub(super) fn clear(&mut self) {
        self.revisions.clear();
        self.actors.clear();
    }
}
