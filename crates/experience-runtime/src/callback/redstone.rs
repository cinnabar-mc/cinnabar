//! Server WIT 0.12's redstone: the power an Experience's `redstone` blocks receive and emit.

use anyhow::Result;

use super::{CallbackRes, Reach, stage};
use crate::host::cinnabar::experience_server::types::WorldError;
use crate::protocol::{BlockPos, Op};

/// The most redstone power.
pub(super) const MAX_POWER: u8 = 15;

impl CallbackRes {
    /// The power that this Experience's redstone block at `pos` receives, as the snapshot holds it.
    pub(crate) fn redstone_power(&mut self, pos: BlockPos) -> Result<Result<u8, WorldError>> {
        self.host_call()?;
        let own = &self.own;
        Ok(self.snapshot.read(pos).and_then(|slot| {
            if !slot.owned {
                Err(WorldError::NotOwned)
            } else if !own.redstone_blocks.contains(&slot.id) {
                Err(WorldError::UnsupportedState)
            } else {
                Ok(slot.power)
            }
        }))
    }

    /// Stages the power that this Experience's redstone block at `pos` emits; a position written
    /// again replaces its staged op.
    pub(crate) fn set_redstone_power(
        &mut self,
        pos: BlockPos,
        power: u8,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        let slot = match self.snapshot.write(pos, Reach::Network) {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !slot.owned {
            return Ok(Err(WorldError::NotOwned));
        }
        if !self.own.redstone_blocks.contains(&slot.id) || power > MAX_POWER {
            return Ok(Err(WorldError::UnsupportedState));
        }
        let op = Op::SetRedstonePower { pos, power };
        match redstone_op(&self.ops, pos) {
            Some(index) => self.ops[index] = op,
            None => stage(&mut self.ops, op)?,
        }
        Ok(Ok(()))
    }
}

/// The index of the staged `SetRedstonePower` at `pos`.
pub(super) fn redstone_op(ops: &[Op], pos: BlockPos) -> Option<usize> {
    ops.iter()
        .position(|op| matches!(op, Op::SetRedstonePower { pos: at, .. } if *at == pos))
}
