//! Replaces fuel-trapped frame guests without replaying their failed transactions.

use super::*;

/// Consecutive frame fuel overruns before a local guest loses its session authority.
pub(crate) const MAX_CONSECUTIVE_FRAME_OVERRUNS: u32 = 3;

impl Instance {
    /// Retains committed output only for bounded, recoverable frame fuel traps.
    pub(super) fn callback_failure(&mut self, fuel: u64, error: anyhow::Error) -> anyhow::Error {
        if fuel == FRAME_FUEL
            && error.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::OutOfFuel)
        {
            self.consecutive_frame_overruns += 1;
            if self.consecutive_frame_overruns < MAX_CONSECUTIVE_FRAME_OVERRUNS {
                self.status = InstanceStatus::Recovering;
                self.store.data().retain_hud_claims();
                return error.context("mod frame fuel exhausted; retaining output until recovery");
            }
        }
        self.quarantine();
        error.context("mod quarantined after a guest trap")
    }

    /// Rebuilds admitted code on the loading lane, preserving committed preferences and inputs.
    /// Returns true only when a fresh guest replaced the trapped component.
    pub(crate) fn recover_frame_fuel(&mut self) -> Result<bool> {
        if self.status != InstanceStatus::Recovering || !self.can_reload() {
            return Ok(false);
        }
        if let Some(budget) = &self.callback_budget {
            anyhow::ensure!(
                budget.reserve(LOAD_FUEL).is_some(),
                "loading callback deferred"
            );
        }
        let previous = self.store.data();
        let mut state = State::new(
            previous.grants.clone(),
            previous.controls.settings().to_owned(),
            previous.declared.clone(),
        );
        state.drawing_policy = previous.drawing_policy;
        if let Some(mut policy) = state.drawing_policy {
            state
                .render
                .retained
                .bind_session(policy.connection, policy.dimension);
            policy.permissions = state.grants.drawing_permissions();
            state.render.masks.bind(policy);
        }
        state.controls.panel = previous
            .controls
            .bundle
            .as_ref()
            .map(|bundle| bundle.panel.clone());
        state.controls.bundle = previous.controls.bundle.clone();
        state.session = Arc::clone(&previous.session);
        state.layout = previous.layout.clone();
        state.exclusive = previous.exclusive.clone();
        let candidate = Self::instantiate(self.pre.clone(), state);
        let mut candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                self.quarantine();
                return Err(error.context("mod quarantined after frame fuel recovery failed"));
            }
        };
        candidate.callback_budget = self.callback_budget.clone();
        candidate.consecutive_frame_overruns = self.consecutive_frame_overruns;
        candidate.set_panel_open(self.panel_open());
        // These host observations were supplied for the upcoming frame, not the failed one.
        std::mem::swap(
            &mut candidate.store.data_mut().player_state,
            &mut self.store.data_mut().player_state,
        );
        candidate.store.data_mut().world.incoming =
            std::mem::take(&mut self.store.data_mut().world.incoming);
        *self = candidate;
        Ok(true)
    }
}
