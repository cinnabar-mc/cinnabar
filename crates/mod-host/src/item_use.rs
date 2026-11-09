use super::{MAX_IMPORT_WRITES, State, cinnabar};
use anyhow::{Result, bail};

/// A use pulse staged by the running frame callback and published only when it succeeds.
#[derive(Default)]
pub(super) struct ItemUsePolicy {
    pending_pulse: bool,
    writes: u32,
    /// The current frame's committed pulse, read before the action filter runs.
    pub pulse: bool,
}

impl ItemUsePolicy {
    /// Publishes a pulse only from a callback that saw a gameplay frame.
    pub fn commit(&mut self, gameplay: bool) {
        self.pulse = std::mem::take(&mut self.pending_pulse) && gameplay;
    }
}

impl cinnabar::extension::item_use::Host for State {
    /// Stages one use press edge while gameplay owns input and the use control is held.
    fn pulse_use(&mut self) -> Result<Result<(), String>> {
        self.item_use_policy.writes += 1;
        if self.item_use_policy.writes > MAX_IMPORT_WRITES {
            bail!("item-use import budget exhausted");
        }
        if !self.grants.item_use {
            return Ok(Err("item-use capability denied".into()));
        }
        if self.snapshot.is_none() || !self.gameplay_facts.use_held {
            return Ok(Err("use pulse requires a held current gameplay use".into()));
        }
        self.item_use_policy.pending_pulse = true;
        Ok(Ok(()))
    }
}
