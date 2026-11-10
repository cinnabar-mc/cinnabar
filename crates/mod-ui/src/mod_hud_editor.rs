//! Native layout editing of bounded HUD previews. Pointer data never enters a guest.

mod actions;
mod caption;
mod data;
mod feedback;
mod history;
mod host;
mod host_geometry;
mod input;
mod inspector;
mod motion;
mod paint;
mod slider;
mod snapping;
mod style;
use style::EditorStyle;
mod template;

#[cfg(test)]
mod amendment_render_tests;

use crate::{mod_panel::transitions::SurfaceTransitions, mod_widgets::template as cards};
use json_ui::{Catalog, Context, DataSource, Scalar, ViewState};
use std::sync::Arc;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS,
    mod_hud::{EditorResult, Hud},
};
use ui_engine::frame::EngineFrame;
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        glass::{GlassRenderer, paint_world_backdrop},
        screen::{CustomRenderer, PaintFrame},
    },
};

const SCREEN: &str = "cinnabar_hud_editor.layout";
const GUIDE_WIDTH: f64 = 0.75;
/// Distance pill height and horizontal text padding, in GUI pixels.
const MEASUREMENT_PILL_HEIGHT: f64 = 9.;
const MEASUREMENT_PILL_PADDING: f64 = 4.;
/// Distance label font scale (about 3.5 px glyphs) and a digit advance that covers it.
const MEASUREMENT_FONT_SCALE: f64 = 0.45;
const MEASUREMENT_DIGIT_ADVANCE: f64 = 3.;
const AXIS_SNAP_DISTANCE: f64 = 6.;
const GEOMETRY_EPSILON: f64 = 0.001;
const EDITOR_MARGIN: f64 = 8.;

/// A host-owned layout transaction sharing preview geometry, snapping, history and editor chrome.
pub struct HudEditor {
    draft: Hud,
    committed: Hud,
    hidden: std::collections::HashSet<String>,
    committed_hidden: std::collections::HashSet<String>,
    history: history::History,
    toast: motion::Toast,
    anchor_motion: Option<motion::AnchorMotion>,
    pub(super) autosave: bool,
    /// Reset intent for the next result; autosave clears it after publication.
    reset: bool,
    history_buttons: bool,
    grid: bool,
    snap: bool,
    bypass_snap: bool,
    data_dirty: bool,
    snap_dirty: bool,
    keep_out: Vec<[f64; 4]>,
    selected: Option<usize>,
    /// Card whose bounds or resize handle was under the pointer at its last event.
    hovered: Option<usize>,
    pub(super) drag: Option<input::Drag>,
    slider: Option<slider::Gesture>,
    pub(super) open: bool,
    result: Option<EditorResult>,
    viewport: [f64; 2],
    inspector: inspector::Placement,
    caption_width: f64,
    caption_scale_width: f64,
    caption_bounds: Option<[f64; 4]>,
    catalog: Option<Arc<Catalog>>,
    screen: CachedScreen,
    data: Arc<DataSource>,
    frame: Option<EngineFrame>,
    view: ViewState,
    transitions: SurfaceTransitions,
}

impl HudEditor {
    /// Removes retained closing visuals immediately when presentation ownership is lost.
    pub(super) fn stop_exit(&mut self) {
        self.transitions.cancel_exit();
        self.frame = None;
    }

    /// Separates logical input ownership from the lifetime of an animated closing surface.
    fn painting(&self) -> bool {
        self.open || self.transitions.exiting()
    }

    /// Samples the caller's presentation clock and releases a completed or disabled exit.
    fn advance_transitions(&mut self, now: f64, animate: bool) {
        if let Some(surface) = &self.draft.surface {
            self.data_dirty |= self.transitions.advance(surface, now, animate);
        }
        if !self.open && (!animate || !self.transitions.active()) {
            self.stop_exit();
        }
    }
}
