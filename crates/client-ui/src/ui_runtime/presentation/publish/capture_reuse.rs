//! Repeats a HUD capture whose inputs did not change.
//!
//! A capture reads the player and UI runtime, whose change ticks the caller vouches for, the
//! world's local actor and item registry, item artwork sources, animated item frames, whether the
//! closed inventory's preparation reads its cells, and, only
//! with a window open or while mounted, block entities and the mount. While none changed, a new
//! capture would store nothing new, so the last one's result stands.

use super::{HudCapture, ItemIconFrames, UiPresentationRuntime, capture_hud_frame};
use crate::ui_runtime::UiRuntime;
use std::sync::Arc;
use ui::IconRef;

/// The last capture's inputs besides the runtimes, and the viewmodel icons it returned.
pub(in crate::ui_runtime::presentation) struct CaptureRecord {
    key: Key,
    icons: (Option<IconRef>, Option<IconRef>),
}

/// What a capture reads besides the player and UI runtime.
struct Key {
    session: u64,
    actor: Option<client_world::ActorPresentationRevision>,
    /// Held, so a replaced registry or catalog never reuses its address.
    registry: Option<Arc<()>>,
    textures: Arc<render_model::UiRenderTextureArray>,
    icon_epoch: u64,
    dials: inventory::DialFrames,
    frames: ItemIconFrames,
    first_person: bool,
    /// Whether the closed inventory's preparation still reads its cells, whose item names the
    /// capture then stores. Publication finishes that preparation without marking anything.
    inventory_inputs: bool,
}

impl Key {
    fn of(
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        presentation: &UiPresentationRuntime,
        world: Option<client_world::ui_read::InventoryUi<'_>>,
        first_person: bool,
        frames: ItemIconFrames,
    ) -> Self {
        Self {
            session: runtime.session_id(),
            actor: world.and_then(|world| world.local_player().presentation_revision()),
            registry: world.map(|world| Arc::clone(world.item_presentation_generation())),
            textures: Arc::clone(&presentation.textures),
            icon_epoch: presentation
                .inventory_cache
                .as_ref()
                .map_or(0, |cache| cache.icon_epoch()),
            dials: presentation.dial_frames(),
            frames,
            first_person,
            inventory_inputs: presentation.inventory_inputs_needed(player_runtime, runtime),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.session == other.session
            && self.actor == other.actor
            && match (&self.registry, &other.registry) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
            && Arc::ptr_eq(&self.textures, &other.textures)
            && self.icon_epoch == other.icon_epoch
            && self.dials == other.dials
            && self.frames == other.frames
            && self.first_person == other.first_person
            && self.inventory_inputs == other.inventory_inputs
    }
}

/// [`capture_hud_frame`], repeating the last capture's result when `runtimes_unchanged`, the
/// caller's word that neither `player_runtime` nor `runtime` changed since the last call, and
/// nothing else the capture reads changed either. A capture with the inventory or a window open,
/// while mounted, or with shield or trim art waiting always runs.
#[allow(clippy::too_many_arguments)]
pub fn capture_hud_frame_if_changed(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    world: Option<client_world::ui_read::InventoryUi<'_>>,
    first_person: bool,
    now_millis: u64,
    icon_frames: ItemIconFrames,
    runtimes_unchanged: bool,
) -> HudCapture {
    let key = Key::of(
        player_runtime,
        runtime,
        presentation,
        world,
        first_person,
        icon_frames,
    );
    let quiet = !runtime.inventory_open()
        && runtime
            .inventory_ledger(player_runtime)
            .window_position()
            .is_none()
        && runtime.gameplay_hud().mount_unique_id().is_none()
        && !presentation.gui_models.shield_icons.dirty()
        && !presentation.gui_models.trim_icons.dirty();
    let last = presentation
        .inventory_cache
        .as_mut()
        .and_then(|cache| cache.hud_capture.take());
    if runtimes_unchanged
        && quiet
        && let Some(last) = last.filter(|last| last.key.matches(&key))
    {
        // The session and local identity observations are cheap and read no revisioned input.
        let mut presentation_changed =
            super::super::dynamic_textures::observe_session(presentation, runtime.session_id());
        presentation_changed |= presentation.capture_local_player_identity(
            world.map(|world| world.local_player()),
            runtime.session_id(),
        );
        #[cfg(debug_assertions)]
        {
            let fresh = capture_hud_frame(
                player_runtime,
                runtime,
                presentation,
                world,
                first_person,
                now_millis,
                icon_frames,
            );
            assert!(
                !fresh.runtime_changed && !fresh.presentation_changed && fresh.icons == last.icons,
                "a repeated HUD capture differs from a fresh one"
            );
        }
        let icons = last.icons;
        if let Some(cache) = presentation.inventory_cache.as_mut() {
            cache.hud_capture = Some(last);
        }
        return HudCapture {
            icons,
            runtime_changed: false,
            presentation_changed,
        };
    }
    let capture = capture_hud_frame(
        player_runtime,
        runtime,
        presentation,
        world,
        first_person,
        now_millis,
        icon_frames,
    );
    #[cfg(test)]
    {
        presentation.hud_captures += 1;
    }
    // The key is taken after the capture, which may refresh the artwork sources it reads.
    let key = Key::of(
        player_runtime,
        runtime,
        presentation,
        world,
        first_person,
        icon_frames,
    );
    if let Some(cache) = presentation.inventory_cache.as_mut() {
        cache.hud_capture = Some(CaptureRecord {
            key,
            icons: capture.icons,
        });
    }
    capture
}

#[cfg(test)]
mod tests;
