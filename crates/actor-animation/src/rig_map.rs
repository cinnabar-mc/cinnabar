//! The store's rigs, stamping a rig's presentation revision whenever it may change.
use super::*;

/// Rigs by lifetime. Reads go through `Deref`; every mutable access gives the rig a new
/// presentation revision, so a reader holding an older revision knows the rig may differ.
/// Setters that run every frame use [`Self::update`] and stamp only when they change a value
/// a snapshot or frame sample can observe.
#[derive(Debug, Default)]
pub(super) struct RigMap {
    rigs: BTreeMap<ActorLifetimeId, ActorRigState>,
}

/// A process-wide presentation stamp. It identifies when a rig or catalog last changed, not
/// what it holds, so debug output omits its value and equal states print alike.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) struct Stamp(pub(super) u64);

impl std::fmt::Debug for Stamp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Stamp")
    }
}

/// A stamp no rig or catalog in any store has held before, so revisions from different stores
/// never compare equal.
pub(super) fn next_revision() -> Stamp {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    Stamp(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

impl std::ops::Deref for RigMap {
    type Target = BTreeMap<ActorLifetimeId, ActorRigState>;

    fn deref(&self) -> &Self::Target {
        &self.rigs
    }
}

impl RigMap {
    /// Borrows one rig for mutation, assuming it changes.
    pub(super) fn get_mut(&mut self, lifetime: &ActorLifetimeId) -> Option<&mut ActorRigState> {
        let state = self.rigs.get_mut(lifetime)?;
        state.presentation_revision = next_revision();
        Some(state)
    }

    /// Runs `update`, stamping the rig only when it reports an observable change.
    pub(super) fn update<R>(
        &mut self,
        lifetime: &ActorLifetimeId,
        update: impl FnOnce(&mut ActorRigState) -> (bool, R),
    ) -> Option<R> {
        let state = self.rigs.get_mut(lifetime)?;
        let (changed, result) = update(state);
        if changed {
            state.presentation_revision = next_revision();
        }
        Some(result)
    }

    /// Borrows every rig for mutation, assuming each changes.
    pub(super) fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (&ActorLifetimeId, &mut ActorRigState)> {
        let revision = next_revision();
        self.rigs.iter_mut().map(move |(lifetime, state)| {
            state.presentation_revision = revision;
            (lifetime, state)
        })
    }

    /// Borrows the rigs `admit` accepts for mutation, assuming each of those changes. Rejected
    /// rigs keep their revision.
    pub(super) fn iter_mut_where(
        &mut self,
        mut admit: impl FnMut(&ActorLifetimeId, &ActorRigState) -> bool,
    ) -> impl Iterator<Item = (&ActorLifetimeId, &mut ActorRigState)> {
        let revision = next_revision();
        self.rigs
            .iter_mut()
            .filter(move |(lifetime, state)| admit(lifetime, state))
            .map(move |(lifetime, state)| {
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
        state.presentation_revision = next_revision();
        self.rigs.insert(lifetime, state);
    }

    pub(super) fn remove(&mut self, lifetime: &ActorLifetimeId) -> Option<ActorRigState> {
        self.rigs.remove(lifetime)
    }

    pub(super) fn clear(&mut self) {
        self.rigs.clear();
    }
}
