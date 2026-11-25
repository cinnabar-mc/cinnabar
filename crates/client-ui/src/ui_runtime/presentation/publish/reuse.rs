//! Repeats the last built frame instead of building it again while nothing it was built from
//! changed.
//!
//! The caller vouches for the revisioned inputs: the UI authority, player state and every
//! presentation input written outside publication. This module checks the rest: the captured
//! publication values, the clock the frame was built at (see [`FrameClock`]) and asynchronous work
//! a build would collect.

use super::super::{UiPresentationRuntime, frame_clock::FrameClock, player_preview};
use super::commit::PendingUiPublication;
#[cfg(debug_assertions)]
use render_model::UiRenderInput;
use std::sync::Arc;
use ui::{DpiScale, IconRef};

/// Whether publication skips frames that would repeat, and whether it checks them. The checking
/// modes exist only in debug builds, so release publication never compares frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameReuse {
    /// Repeat the last frame without building it.
    Skip,
    /// Build anyway and report a frame that differs from the one it should have repeated.
    #[cfg(debug_assertions)]
    Verify,
    /// Repeat as [`Self::Skip`] does, but build every [`AUDIT_PERIOD`]th repeat in a row and
    /// report a frame that differs from the repeated one: a repeat's replayed bookkeeping that
    /// diverged from what a build would have done shows there.
    #[cfg(debug_assertions)]
    Audit,
    /// Always build.
    Off,
}

/// Consecutive repeats [`FrameReuse::Audit`] lets pass before it builds one to compare.
#[cfg(debug_assertions)]
pub const AUDIT_PERIOD: u32 = 16;

impl Default for FrameReuse {
    /// Debug builds audit repeats; others skip.
    fn default() -> Self {
        #[cfg(debug_assertions)]
        return Self::Audit;
        #[cfg(not(debug_assertions))]
        return Self::Skip;
    }
}

/// The captured publication values a build reads besides its revisioned inputs.
#[derive(PartialEq)]
pub(in crate::ui_runtime::presentation) struct CapturedInputs {
    skin: Option<Arc<[u8]>>,
    preview: (bool, player_preview::PlayerPreviewPose, bool, bool),
    item_icons: (Option<IconRef>, Option<IconRef>),
    physical_size: [u32; 2],
    dpi_scale: DpiScale,
}

impl CapturedInputs {
    pub(in crate::ui_runtime::presentation) fn of(prepared: &PendingUiPublication) -> Self {
        let preview = &prepared.preview;
        Self {
            skin: preview.skin.clone(),
            preview: (preview.ready, preview.pose, preview.shown, preview.hands),
            item_icons: prepared.item_icons,
            physical_size: prepared.physical_size,
            dpi_scale: prepared.dpi_scale,
        }
    }
}

/// The last successfully built frame's captured inputs and the clock it read.
pub(in crate::ui_runtime::presentation) struct BuiltFrame {
    inputs: CapturedInputs,
    clock: FrameClock,
    /// The player model revision it drew, when it drew the model.
    player_model: Option<u64>,
}

impl UiPresentationRuntime {
    /// Sets whether publication skips repeated frames.
    pub fn set_frame_reuse(&mut self, reuse: FrameReuse) {
        self.frame_reuse = reuse;
    }

    /// Frames publication built rather than repeated.
    #[cfg(any(test, feature = "test-support"))]
    pub fn frames_built(&self) -> u64 {
        self.frames_built
    }

    /// Repeated frames whose check build drew something else.
    pub fn frame_reuse_mismatches(&self) -> u64 {
        self.frame_reuse_mismatches
    }

    /// Whether the last built frame would be built again exactly from `inputs` at
    /// `now_millis`, given unchanged revisioned inputs. Collects completed asynchronous work
    /// first, as a build would.
    pub(super) fn repeats_frame(&mut self, inputs: &CapturedInputs, now_millis: u64) -> bool {
        let holds = self.built_frame.as_ref().is_some_and(|built| {
            built.inputs == *inputs
                && built.clock.holds_at(now_millis)
                && built
                    .player_model
                    .is_none_or(|revision| revision == self.player_model_revision)
        });
        holds && !self.collect_async_work()
    }

    /// Collects finished background work a build reads; whether any arrived, waits or still
    /// needs builds to finish.
    fn collect_async_work(&mut self) -> bool {
        // Evaluated in full: each poll installs what it collected, as the build would.
        let artwork = self.menu_artwork_loader.poll();
        let images = self.poll_server_ui_images();
        let decodes = self
            .engine
            .as_deref()
            .is_some_and(ui_engine::engine::FormEngine::texture_completions_waiting);
        // Closed-inventory preparation runs on a worker that only builds service; it finishes
        // during idle frames rather than on the frame that opens the inventory.
        let preparing = self.engine_inventory_preparation_pending();
        artwork | images | decodes | preparing | self.session_icons.packing()
    }

    /// Records the frame just built from `inputs` for later repeats.
    pub(super) fn remember_built(&mut self, inputs: CapturedInputs) {
        self.built_frame = Some(BuiltFrame {
            inputs,
            clock: self.clock.clone(),
            player_model: self
                .player_model_drawn()
                .then_some(self.player_model_revision),
        });
    }

    /// Forgets the built frame, so the next publication builds, as after a rejected one.
    pub fn forget_built_frame(&mut self) {
        self.built_frame = None;
    }

    /// Does what building the repeated frame at `now_millis` would have done besides drawing it;
    /// `false` when the frame would draw differently after all, so it must be built.
    pub(super) fn repeat_built_frame(&mut self, now_millis: u64) -> bool {
        if !self.repeat_hud_paints(now_millis) {
            return false;
        }
        // The replay touched the paints' animation components as their paints would; a build
        // ends its frame by retiring what nothing touched, and so does a repeat.
        self.end_animation_frame();
        true
    }

    /// Whether this repeated frame is built and compared instead: always when verifying, every
    /// [`AUDIT_PERIOD`]th repeat in a row when auditing, never otherwise.
    #[cfg(debug_assertions)]
    pub(super) fn checks_repeat(&mut self) -> bool {
        match self.frame_reuse {
            FrameReuse::Verify => true,
            FrameReuse::Audit => {
                self.repeats_since_check += 1;
                let due = self.repeats_since_check >= AUDIT_PERIOD;
                if due {
                    self.repeats_since_check = 0;
                }
                due
            }
            FrameReuse::Skip | FrameReuse::Off => false,
        }
    }

    /// Reports a check build that drew something other than the frame it should have repeated.
    #[cfg(debug_assertions)]
    pub(super) fn check_repeat(&mut self, repeated: Option<&UiRenderInput>, built: &UiRenderInput) {
        if repeated == Some(built) {
            return;
        }
        self.frame_reuse_mismatches += 1;
        let changed_by = self.clock.changed_by();
        tracing::error!(
            mismatches = self.frame_reuse_mismatches,
            ?changed_by,
            "a UI frame published as unchanged differs from a fresh build: an input is missing \
             its revision"
        );
        #[cfg(any(test, feature = "test-support"))]
        panic!("a UI frame published as unchanged differs from a fresh build");
    }
}
