//! Callback transaction boundaries and publication into current native state.

use super::*;

impl State {
    /// Starts counters and immutable clocks exactly once per invocation, including post-return.
    pub(super) fn begin_callback(&mut self) {
        let state = self;
        state.writes = 0;
        state.environment.begin_callback();
        state.gameplay_reads = 0;
        state.camera_writes = 0;
        state.player_state.begin_callback();
        state.motion.begin_callback();
        state.packet_delay_writes = 0;
        state.controls.reset_budget();
        state.calls = 0;
        state.output = 0;
        state.pending_screens = None;
        state.inventory.begin();
        state.hud.begin_callback();
        state.render.begin_callback();
        state.touch.begin_callback();
        state.presentation.begin();
        state.facts.begin();
        let now = chrono::Local::now();
        state.clock = Some(cinnabar::extension::clock::LocalTime {
            unix_ms: u64::try_from(now.timestamp_millis()).unwrap_or(0),
            utc_offset_seconds: now.offset().local_minus_utc(),
        });
        state.events.elapsed = Some(state.events.started.elapsed().as_secs_f64());
    }

    /// Moves guest writes into current host owners, preserving native observations and queues.
    /// Exhaustive matching makes every new import owner choose its publication policy.
    pub(super) fn accept_frame(
        &mut self,
        transaction: Self,
        current_frame: bool,
        same_ui: bool,
        refused_before: u64,
    ) {
        let Self {
            exclusive,
            terrain,
            limits: _,
            pressed: _,
            label: _,
            hud,
            presentation,
            facts,
            pending,
            writes: _,
            grants: _,
            environment,
            motion,
            snapshot: _,
            gameplay_reads: _,
            camera_writes: _,
            pending_camera,
            camera_delta: _,
            packet_delay_ms: _,
            pending_packet_delay,
            packet_delay_writes: _,
            gameplay_facts: _,
            controls,
            world,
            camera_policy,
            item_use_policy,
            player_state,
            render,
            drawing_policy: _,
            presentation_epoch: _,
            asset_files: _,
            declared: _,
            layout: _,
            session: _,
            screens: _,
            mut pending_screens,
            calls: _,
            output: _,
            inventory,
            events: _,
            touch,
            clock: _,
        } = transaction;
        self.exclusive.refused = self
            .exclusive
            .refused
            .saturating_add(exclusive.refused.saturating_sub(refused_before));
        self.pending = pending;
        self.hud.accept(hud);
        self.presentation.accept(presentation);
        self.controls.accept(controls, current_frame, same_ui);
        self.touch.accept(touch);
        self.terrain.accept(terrain);
        self.world
            .accept(world, current_frame, self.exclusive.camera);
        self.inventory.accept(inventory, current_frame);
        self.environment
            .accept(environment, self.exclusive.environment);
        self.player_state.accept_reads(player_state);
        self.facts.accept_reads(facts);
        // Drawing owns both tokens and staging, and must reconcile current actor lifetimes.
        self.render.accept(render);
        if !current_frame && let Some(screens) = &mut pending_screens {
            screens.expire_focus();
        }
        self.pending_screens = if same_ui { pending_screens } else { None };
        if current_frame {
            self.motion.pending_jump = motion.pending_jump;
            self.motion.pending_jump_cancel = motion.pending_jump_cancel;
            if !self.exclusive.camera {
                self.pending_camera = pending_camera;
                self.camera_policy = camera_policy;
            }
            self.pending_packet_delay = pending_packet_delay;
            self.item_use_policy = item_use_policy;
        }
    }
}

/// Publishes retained presentation changes after the entire callback succeeds.
pub(super) fn commit(store: &mut Store<State>) {
    commit_frame_outputs(store, false);
}

/// Finishes a frame whose drawing may already have reconciled native observations.
pub(super) fn commit_frame_outputs(store: &mut Store<State>, drawing_committed: bool) {
    let state = store.data_mut();
    state.motion.commit_frame();
    state.terrain.commit();
    // Retained drawing is committed once, by `commit_event`, so frame groups survive.
    if !drawing_committed {
        state.render.commit_primitives();
    }
    state.world.commit();
    state.camera_policy.committed_view_scale = (state.snapshot.is_some()
        && state.controls.frame.focused
        && !state.controls.frame.panel_open)
        .then_some(state.camera_policy.pending_view_scale)
        .flatten();
    state.camera_policy.committed_view_tilt = state.camera_policy.pending_view_tilt;
    state.item_use_policy.commit(state.snapshot.is_some());
    state.camera_delta = state.pending_camera.take();
    state.exclusive.camera_claimed = state.camera_output();
    if let Some(delay) = state.pending_packet_delay.take() {
        state.packet_delay_ms = delay;
    }
    commit_event_outputs(store.data_mut(), drawing_committed);
}

/// Publishes retained drawing, label, visual time, panel, settings and screen changes.
/// Immediate primitives, camera and command output belong to `frame` alone;
/// an event's staged changes to those outputs are dropped by the next frame.
pub(super) fn commit_event(store: &mut Store<State>) {
    commit_event_outputs(store.data_mut(), false);
}

/// Commits retained owners once after a complete call and post-return.
fn commit_event_outputs(state: &mut State, drawing_committed: bool) {
    if !state.world_output_available() {
        state.clear_world_output();
    }
    if !drawing_committed {
        state.render.commit_retained();
    }
    state.hud.commit();
    state.presentation.commit();
    state.controls.commit();
    state.touch.commit();
    state.inventory.commit();
    state.environment.commit();
    if let Some(text) = state.pending.take() {
        state.label = (!text.is_empty()).then_some(text);
    }
    if let Some(screens) = state
        .pending_screens
        .take()
        .and_then(player_mod::ScreenTransaction::commit)
    {
        state.screens = screens;
    }
}
