//! World output has a shorter lifetime than the guest's memory and local UI.

use super::State;

/// Makes world and actor anchors unavailable while preserving projected control semantics.
pub(super) fn clear_surface_world(surface: &mut ui::mod_panel::Surface) {
    for value in surface.bindings.values_mut() {
        value.invalidate_projection();
    }
}

impl State {
    /// Unbound standalone hosts keep their caller-managed lifecycle; a bound loading scope
    /// cannot publish world output, including from init, events or fuel recovery.
    pub(super) fn world_output_available(&self) -> bool {
        self.drawing_policy
            .is_none_or(|session| session.connection != 0)
    }

    /// Discards staged and committed world output together. New state fields require an
    /// explicit lifetime decision here; local UI, preferences and input routing survive.
    pub(super) fn clear_world_output(&mut self) {
        let Self {
            exclusive,
            terrain: _,
            limits: _,
            pressed: _,
            label: _,
            hud,
            presentation,
            facts: _,
            pending: _,
            writes: _,
            grants: _,
            environment,
            motion,
            snapshot: _,
            gameplay_reads: _,
            camera_writes: _,
            pending_camera,
            camera_delta,
            packet_delay_ms,
            pending_packet_delay,
            packet_delay_writes: _,
            gameplay_facts: _,
            controls,
            world,
            camera_policy,
            item_use_policy,
            player_state: _,
            render,
            drawing_policy: _,
            presentation_epoch: _,
            asset_files: _,
            declared: _,
            layout: _,
            session: _,
            screens: _,
            pending_screens: _,
            calls: _,
            output: _,
            inventory,
            events: _,
            touch: _,
        } = self;
        environment.revoke();
        motion.revoke();
        *world = Default::default();
        *pending_camera = None;
        *camera_delta = None;
        *camera_policy = Default::default();
        *item_use_policy = Default::default();
        inventory.revoke();
        *packet_delay_ms = 0;
        *pending_packet_delay = None;
        exclusive.camera_claimed = false;
        controls.interaction = Default::default();
        controls.pending_interaction = Default::default();
        controls.clear_world_projections();
        hud.clear_world_projections();
        render.revoke();
        presentation.clear_world();
    }
}
