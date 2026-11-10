//! Input-frame identity and recovery after an incomplete physical sample.

use semantic_input::{
    Action,
    identity::{InputAuthorityGeneration, InputSequence},
};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// Authorities that scope input and accepted gameplay work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionScope {
    pub session: SessionGeneration,
    pub position: PositionAuthorityGeneration,
    pub dimension: i32,
    pub input: InputAuthorityGeneration,
}

/// Separates input cancellation, changed geometry and world replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalidation {
    None,
    Input,
    /// Retires old geometry while preserving accepted item-use charges and releases.
    Position,
    /// Retires accepted work from the previous session or dimension.
    World,
}

/// Observes input authority and the active build action without storing pending attempts.
#[derive(Debug, Default)]
pub struct OrderedInteractions {
    scope: Option<InteractionScope>,
    frame: Option<(InteractionScope, InputSequence)>,
    quarantined: bool,
    build_action: Option<Action>,
}

impl OrderedInteractions {
    /// Reports the strongest changed authority so each owner can retire its own work.
    pub fn synchronize(&mut self, scope: InteractionScope, invalidated: bool) -> Invalidation {
        if invalidated
            || self
                .scope
                .is_some_and(|previous| previous.input != scope.input)
        {
            self.build_action = None;
        }
        let world_changed = self.scope.is_some_and(|previous| {
            (previous.session, previous.dimension) != (scope.session, scope.dimension)
        });
        let position_changed = self
            .scope
            .is_some_and(|previous| previous.position != scope.position);
        let changed = self.scope.is_some_and(|previous| previous != scope);
        self.scope = Some(scope);
        if world_changed {
            Invalidation::World
        } else if position_changed {
            Invalidation::Position
        } else if changed || invalidated {
            Invalidation::Input
        } else {
            Invalidation::None
        }
    }

    /// Whether `session` and `dimension` differ from the synchronized world, so
    /// the next synchronization will invalidate world-owned work.
    pub fn changes_world(&self, session: SessionGeneration, dimension: i32) -> bool {
        self.scope
            .is_some_and(|scope| (scope.session, scope.dimension) != (session, dimension))
    }

    /// Claims a frame once so repeated producer passes cannot replay its edges.
    pub fn observe_frame(&mut self, scope: InteractionScope, frame: InputSequence) -> bool {
        if self.frame == Some((scope, frame)) {
            return false;
        }
        self.frame = Some((scope, frame));
        true
    }

    /// Suppresses gestures after a gap whose missing transitions cannot be recovered.
    pub fn quarantine(&mut self) {
        self.quarantined = true;
        self.build_action = None;
    }

    /// Rearms only after a complete neutral sample, so a missing release cannot stick.
    pub fn observe_neutral(&mut self, neutral: bool) {
        if neutral {
            self.quarantined = false;
            self.build_action = None;
        }
    }

    /// Whether a physical input gap still prevents new gestures.
    pub const fn quarantined(&self) -> bool {
        self.quarantined
    }

    /// Replaces the mining or placement intention on a press; a release clears it.
    /// Physical held buttons never restore an earlier intention.
    pub fn observe_build_button(&mut self, action: Action, pressed: bool) {
        if matches!(action, Action::Attack | Action::Use) {
            self.build_action = pressed.then_some(action);
        }
    }

    /// Whether this action owns held mining or placement at the consumed input cursor.
    pub fn build_action_active(&self, action: Action) -> bool {
        !self.quarantined && self.build_action == Some(action)
    }
}

#[cfg(test)]
mod tests;
