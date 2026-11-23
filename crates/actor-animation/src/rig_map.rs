//! The store's rigs, stamping a rig's presentation revision whenever it may change.
use super::*;

/// Rigs by lifetime. Reads go through `Deref`; every mutable access gives the rig a new
/// presentation revision, so a reader holding an older revision knows the rig may differ.
/// Setters that run every frame use [`Self::update`] and stamp only when they change a value
/// a snapshot or frame sample can observe.
#[derive(Debug, Default)]
pub(super) struct RigMap {
    rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
    last_revision: u64,
}

impl std::ops::Deref for RigMap {
    type Target = BTreeMap<ActorLifetimeId, ActorRigState>;

    fn deref(&self) -> &Self::Target {
        &self.rigs
    }
}

impl RigMap {
    /// A revision no rig has held before.
    fn next_revision(&mut self) -> u64 {
        self.last_revision = self.last_revision.wrapping_add(1).max(1);
        self.last_revision
    }

    /// Borrows one rig for mutation, assuming it changes.
    pub(super) fn get_mut(&mut self, lifetime: &ActorLifetimeId) -> Option<&mut ActorRigState> {
        let revision = self.next_revision();
        let state = self.rigs.get_mut(lifetime)?;
        state.presentation_revision = revision;
        Some(state)
    }

    /// Runs `update`, stamping the rig only when it reports an observable change.
    pub(super) fn update<R>(
        &mut self,
        lifetime: &ActorLifetimeId,
        update: impl FnOnce(&mut ActorRigState) -> (bool, R),
    ) -> Option<R> {
        let revision = self.last_revision.wrapping_add(1).max(1);
        let state = self.rigs.get_mut(lifetime)?;
        let (changed, result) = update(state);
        if changed {
            state.presentation_revision = revision;
            self.last_revision = revision;
        }
        Some(result)
    }

    /// Borrows every rig for mutation, assuming each changes.
    pub(super) fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (&ActorLifetimeId, &mut ActorRigState)> {
        let revision = self.next_revision();
        self.rigs.iter_mut().map(move |(lifetime, state)| {
            state.presentation_revision = revision;
            (lifetime, state)
        })
    }

    /// Borrows every rig for mutation, assuming each changes.
    #[cfg(test)]
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut ActorRigState> {
        self.iter_mut().map(|(_, state)| state)
    }

    pub(super) fn insert(&mut self, lifetime: ActorLifetimeId, mut state: ActorRigState) {
        state.presentation_revision = self.next_revision();
        self.rigs.insert(lifetime, state);
    }

    pub(super) fn remove(&mut self, lifetime: &ActorLifetimeId) -> Option<ActorRigState> {
        self.rigs.remove(lifetime)
    }

    pub(super) fn clear(&mut self) {
        self.rigs.clear();
    }
}
