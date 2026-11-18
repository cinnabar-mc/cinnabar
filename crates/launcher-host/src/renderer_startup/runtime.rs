//! Reuses startup recovery when a running renderer reports a lost device.

use super::*;

impl StartupGuard {
    /// Excludes this session's lost device API on restart, without changing newer settings
    /// or another client's live startup. Call only for a confirmed device loss.
    pub fn device_lost(&mut self) -> Result<()> {
        let _active = termination::active()?;
        let Some(attempt) = self.pending.as_ref().or(self.completed.as_ref()).cloned() else {
            return Ok(());
        };
        // First presentation releases the lease; runtime failures must acquire it again.
        let _lease = if self.lock.is_none() {
            Some(lease(&self.root)?)
        } else {
            None
        };
        let mut recovery = Self::saved(&self.root)?;
        if recovery.version != self.recovery.version
            || recovery.graphics_api_revision != self.recovery.graphics_api_revision
        {
            return Ok(());
        }
        recovery.recover(&attempt);
        save(&self.root.join(CHOICE_FILE), &recovery)?;
        if self.pending.is_some() {
            fs::remove_file(self.root.join(SENTINEL_FILE))
                .context("clear lost renderer startup sentinel")?;
        }
        crash_state::set_renderer(&self.root, Some(&attempt));
        self.recovery = recovery;
        self.pending = None;
        self.completed = None;
        self.lock = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
