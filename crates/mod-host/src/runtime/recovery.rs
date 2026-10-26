//! Replaces fuel-trapped frame guests without replaying their failed transactions.

use super::*;

impl Instance {
    /// Retains committed output only for bounded, recoverable frame fuel traps.
    pub(super) fn callback_failure(&mut self, fuel: u64, error: anyhow::Error) -> anyhow::Error {
        if fuel == FRAME_FUEL
            && error.downcast_ref::<wasmtime::Trap>() == Some(&wasmtime::Trap::OutOfFuel)
        {
            self.consecutive_frame_overruns += 1;
            if self.consecutive_frame_overruns < MAX_CONSECUTIVE_FRAME_OVERRUNS {
                self.status = InstanceStatus::Recovering;
                return error.context("mod frame fuel exhausted; retaining output until recovery");
            }
        }
        self.quarantine();
        error.context("mod quarantined after a guest trap")
    }

    /// Moves complete host state into a fresh store and reinitializes admitted guest code.
    /// Returns true only after the bounded loading lane admitted a successful restart.
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
        self.begin_frame();
        self.store.data_mut().rollback_callback();
        self.store.data_mut().render.restart_guest();
        let open = self.panel_open();
        // This placeholder is never entered or published; both result paths restore real state.
        let placeholder = Store::new(
            self.pre.engine(),
            State::new(ModGrants::default(), String::new(), Declared::default()),
        );
        let previous = std::mem::replace(&mut self.store, placeholder);
        self.store = Store::new(self.pre.engine(), previous.into_data());
        match Self::initialize(&self.pre, &mut self.store) {
            Ok((exports, fuel)) => {
                self.exports = exports;
                self.last_fuel = fuel;
                self.status = InstanceStatus::Running;
                self.filter_identity = super::action_filter::next_identity();
            }
            Err(error) => {
                self.quarantine();
                return Err(error.context("mod quarantined after frame fuel recovery failed"));
            }
        }
        self.set_panel_open(open);
        Ok(true)
    }
}

impl State {
    /// Drops failed retained writes after `begin_frame` clears frame-local staging.
    /// Exhaustive matching requires an explicit recovery decision for new host fields.
    fn rollback_callback(&mut self) {
        let Self {
            exclusive: _,
            terrain: _,
            limits: _,
            pressed: _,
            label: _,
            hud,
            presentation,
            facts,
            pending,
            writes,
            grants: _,
            environment,
            motion: _,
            snapshot: _,
            gameplay_reads,
            camera_writes,
            pending_camera: _,
            camera_delta: _,
            packet_delay_ms: _,
            pending_packet_delay: _,
            packet_delay_writes,
            gameplay_facts: _,
            controls,
            world: _,
            camera_policy: _,
            item_use_policy: _,
            player_state: _,
            render: _,
            drawing_policy: _,
            presentation_epoch: _,
            asset_files: _,
            declared: _,
            layout: _,
            session: _,
            screens: _,
            pending_screens,
            calls,
            output,
            inventory,
            events: _,
            touch,
        } = self;
        *pending = None;
        *pending_screens = None;
        *writes = 0;
        *gameplay_reads = 0;
        *camera_writes = 0;
        *packet_delay_writes = 0;
        *calls = 0;
        *output = 0;
        hud.rollback();
        presentation.begin();
        facts.begin();
        controls.rollback();
        environment.rollback();
        inventory.begin();
        touch.begin_callback();
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
