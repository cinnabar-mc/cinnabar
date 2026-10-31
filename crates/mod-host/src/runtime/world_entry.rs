//! One host-owned allowance per observed world session, independent of guest memory.

use super::State;

#[derive(Clone, Copy, Default)]
pub(super) struct WorldEntry {
    session: Option<u64>,
    pending: bool,
}

impl WorldEntry {
    /// Arms only on a new validated session; temporary absence cannot renew an allowance.
    pub(super) fn observe(&mut self, session: Option<u64>) {
        if let Some(session) = session
            && self.session != Some(session)
        {
            self.session = Some(session);
            self.pending = true;
        }
    }

    /// Requires the current world to match the pending host observation and drawing scope.
    pub(super) fn available(&self, state: &State) -> bool {
        self.pending
            && state.world_output_available()
            && state
                .player_state
                .world
                .as_ref()
                .is_some_and(|world| Some(world.session) == self.session)
    }

    /// Spends the allowance before entering guest code, including callbacks that trap.
    pub(super) fn consume(&mut self) {
        self.pending = false;
    }
}
