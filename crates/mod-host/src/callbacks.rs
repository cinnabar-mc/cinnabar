//! Bounded local guest callbacks and the observations and outputs they exchange.

use super::*;
use anyhow::Result;

impl ModHost {
    /// Resets frame-local outputs before input or lifecycle callbacks, preserving retained state.
    pub fn begin_frame(&mut self) {
        self.instance.begin_frame();
    }

    /// Whether a superseded authority transition requires a fresh instance.
    pub fn is_revoked(&self) -> bool {
        self.instance.is_revoked()
    }

    /// Whether a frame fuel trap is waiting for fresh guest initialization.
    pub fn is_recovering(&self) -> bool {
        self.instance.is_recovering()
    }

    /// Runs a bounded callback; isolated frame fuel traps retain output and recover next frame.
    pub fn frame(&mut self, pressed: bool) -> Result<()> {
        self.frame_with_gameplay(pressed, None)
    }

    /// Runs a callback with a validated snapshot belonging only to this frame.
    pub fn frame_with_gameplay(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
    ) -> Result<()> {
        self.frame_with_controls(pressed, snapshot, empty_controls())
    }

    /// Receives only bounded host-owned edges, alongside the current gameplay frame.
    pub fn frame_with_controls(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.frame_with_world(pressed, snapshot, Vec::new(), controls)
    }

    /// Adds nearby mobs, readable only with the entities grant and a current snapshot.
    pub fn frame_with_world(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.frame_with_player_state(pressed, snapshot, mobs, None, controls)
    }

    /// Supplies read-only local facts independently of captured gameplay input.
    pub fn frame_with_player_state(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        player_state: Option<PlayerStateSnapshot>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.frame_with_movement(pressed, snapshot, mobs, player_state, None, controls)
    }

    /// Shares frame fuel; the booleans say whether a higher mod owns camera or time output.
    pub fn set_frame_authority(
        &mut self,
        budget: CallbackBudget,
        camera: bool,
        environment: bool,
        owner: usize,
    ) {
        self.instance
            .set_frame_authority(budget, camera, environment, owner);
    }

    /// Whether this instance currently publishes exclusive camera output in this frame.
    pub fn has_camera_output(&self) -> bool {
        self.instance.has_camera_output()
    }

    /// Counts output attempts refused because a higher active mod owns that output.
    pub fn refused_outputs(&self) -> u64 {
        self.instance.refused_outputs()
    }

    /// Adds local physics state for this callback, under movement authority.
    pub fn frame_with_movement(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        player_state: Option<PlayerStateSnapshot>,
        movement: Option<GameplayMovementSnapshot>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.begin_frame();
        let result = self.recover_frame_fuel().and_then(|recovered| {
            if recovered {
                // Input was sampled from the old UI; resume after the initialized UI is shown.
                Ok(())
            } else {
                match self.dispatch(Vec::new()) {
                    Ok(delivery) => self
                        .instance
                        .observe_frame(pressed, snapshot, mobs, player_state, movement, controls)
                        .and_then(|()| self.instance.frame(delivery)),
                    Err(error) => Err(error),
                }
            }
        });
        self.instance.finish_frame();
        self.queue_settings();
        result
    }

    /// Restarts a fuel-trapped guest and restores its lifecycle context.
    /// Returns true when input sampled before replacement must be discarded.
    fn recover_frame_fuel(&mut self) -> Result<bool> {
        let recovered = self.instance.recover_frame_fuel()?;
        if recovered {
            self.pending_events
                .retain(|event| !matches!(event, ModEvent::TextChanged { .. }));
        }
        if recovered && self.instance.has_events() {
            self.pending_events
                .insert(0, ModEvent::ScreenChanged(self.layout.clone()));
            self.pending_events.insert(
                1,
                ModEvent::DataChanged(vec![DataSource::Items, DataSource::Recipes]),
            );
        }
        Ok(recovered)
    }

    /// Supplies bounded, read-only presentation facts for the next frame callback only.
    pub fn observe_presentation(
        &mut self,
        world: Option<WorldObservation>,
        fps: Option<f32>,
        sidebar: Option<SidebarSnapshot>,
        chat: Vec<ObservedChatMessage>,
    ) -> Result<()> {
        self.instance
            .observe_presentation(world, fps, sidebar, chat)
    }

    /// Supplies bounded, session-fenced cosmetic facts for the next callback only.
    pub fn observe_modules(
        &mut self,
        observations: module_observations::ModuleObservations,
    ) -> Result<()> {
        self.instance.observe_modules(observations)
    }

    /// Supplies client facts for the next frame callback only; each import checks its grant.
    pub fn observe_gameplay(&mut self, facts: GameplayFacts) -> Result<()> {
        self.instance.observe_gameplay(facts)
    }

    /// Whether the last successful gameplay callback opted in to preserving teleport aim.
    pub fn preserves_teleport_rotation(&self) -> bool {
        self.instance.preserves_teleport_rotation()
    }

    /// Current-frame FOV and look multipliers; absence restores neutral 1/1.
    pub fn camera_view_scale(&self) -> Option<[f32; 2]> {
        self.instance.camera_view_scale()
    }

    /// Current-frame view tilt; absence restores the native view and damage shake.
    pub fn camera_view_tilt(&self) -> Option<mod_api::native_camera::ViewTilt> {
        self.instance.camera_view_tilt()
    }

    /// Consumes a successful callback's jump request once.
    pub fn take_jump_pulse(&mut self) -> bool {
        self.instance.take_jump_pulse()
    }

    /// Consumes an explicit cancellation of pending jump input once.
    pub fn take_jump_cancel(&mut self) -> bool {
        self.instance.take_jump_cancel()
    }

    /// The retained camera rig from the last successful callback.
    pub fn camera_rig(&self) -> Option<GameplayCameraRig> {
        self.instance.camera_rig()
    }

    /// Consumes the last successful frame's granted command requests once.
    pub fn take_commands(&mut self) -> Vec<String> {
        self.instance.take_commands()
    }

    /// Consumes the committed vanilla inventory requests once.
    pub fn take_inventory_requests(&mut self) -> Vec<crate::InventoryRequest> {
        self.instance.take_inventory_requests()
    }

    /// Cues the next callback can poll, typically last frame's from every loaded mod.
    pub fn deliver_cues(&mut self, cues: Vec<ModCue>) {
        self.instance.deliver_cues(cues);
    }

    /// Consumes committed local-only system feedback once.
    pub fn take_local_messages(&mut self) -> Vec<String> {
        self.instance.take_local_messages()
    }

    /// Consumes the last successful frame's presentation cues once.
    pub fn take_cues(&mut self) -> Vec<ModCue> {
        self.instance.take_cues()
    }

    /// Queues committed preferences on the active persistence lane without waiting for disk.
    pub(super) fn queue_settings(&mut self) {
        if let Some(writer) = &self.settings_writer
            && writer.is_active()
            && let Some(json) = self.instance.settings_write()
        {
            writer.submit(json.to_owned());
            self.instance.settings_written();
        }
    }

    /// Consumes an asynchronous persistence error without quarantining the guest.
    pub fn take_settings_error(&self) -> Option<String> {
        self.settings_writer
            .as_ref()
            .and_then(settings::SettingsWriter::take_error)
    }

    /// Retained request from a successful callback, independent of UI focus.
    pub fn packet_delay_ms(&self) -> u32 {
        self.instance.packet_delay_ms()
    }

    /// Whether this mod retains authority and can resume callbacks.
    pub fn is_active(&self) -> bool {
        self.instance.is_active()
    }
}
