//! Server WIT 0.12's actor damage: the one hurt a result gives its actor.

use anyhow::Result;

use super::{CallbackRes, stage};
use crate::host::cinnabar::experience_server::types::WorldError;
use crate::protocol::Op;

impl CallbackRes {
    /// Stages `amount` of damage to `player`, who must be the actor; it replaces any damage
    /// staged before it.
    pub(crate) fn damage(
        &mut self,
        player: String,
        amount: f64,
        reduced_by_armour: bool,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if let Err(error) = self.actor_is(&player) {
            return Ok(Err(error));
        }
        if !amount.is_finite() || amount < 0.0 {
            return Ok(Err(WorldError::OutOfBounds));
        }
        let op = Op::Damage {
            player,
            amount,
            reduced_by_armour,
        };
        match self
            .ops
            .iter()
            .position(|op| matches!(op, Op::Damage { .. }))
        {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        Ok(Ok(()))
    }
}
