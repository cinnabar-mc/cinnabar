//! Server WIT 0.12's actor motion: a velocity or knockback the result gives its actor.

use anyhow::Result;

use super::{CallbackRes, stage};
use crate::host::cinnabar::experience_server::types::WorldError;
use crate::load::entities::finite;
use crate::protocol::{Op, Position};

impl CallbackRes {
    /// Stages a new velocity for `player`, who must be the actor; it replaces any motion staged
    /// before it.
    pub(crate) fn set_velocity(
        &mut self,
        player: String,
        velocity: Position,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.actor_is(&player) {
            return Ok(Err(error));
        }
        if !finite(velocity) {
            return Ok(Err(WorldError::OutOfBounds));
        }
        self.stage_motion(Op::SetVelocity { player, velocity })
    }

    /// Stages a knockback of `player`, who must be the actor, away from `source`; it replaces any
    /// motion staged before it.
    pub(crate) fn knock_back(
        &mut self,
        player: String,
        source: Position,
        force: f64,
        height: f64,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.actor_is(&player) {
            return Ok(Err(error));
        }
        if !finite(source) || !force.is_finite() || !height.is_finite() {
            return Ok(Err(WorldError::OutOfBounds));
        }
        self.stage_motion(Op::KnockBack {
            player,
            source,
            force,
            height,
        })
    }

    /// Stages `op` as the result's one motion, in place of an earlier one.
    fn stage_motion(&mut self, op: Op) -> Result<Result<(), WorldError>> {
        match motion_op(&self.ops) {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        Ok(Ok(()))
    }
}

/// The index of the staged `SetVelocity` or `KnockBack`, of which a result holds at most one.
fn motion_op(ops: &[Op]) -> Option<usize> {
    ops.iter()
        .position(|op| matches!(op, Op::SetVelocity { .. } | Op::KnockBack { .. }))
}
