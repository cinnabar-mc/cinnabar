//! The launcher text boxes' caret, after vanilla's text edit component as the
//! engine models it (`json_ui` `component/edit.rs` and `emit.rs`): the
//! `CARET_GLYPH` at the caret, toggled every `CARET_BLINK_SECONDS` and shown
//! again after each edit. A press inside a box places the caret at the
//! character nearest the pointer, measured as the box drew its text.

use ui::UiPoint;

use super::super::UiPresentationRuntime;
use launcher::menu::{MenuField, MenuView};
use ui_engine::caret::{TextSpot, nearest_boundary};

/// The caret's blink clock and where the last menu frame drew each box's text.
#[derive(Default)]
pub(super) struct MenuCaretState {
    blink: CaretBlink,
    spots: Vec<TextSpot<MenuField>>,
}

/// A caret's blink clock: shown for `CARET_BLINK_SECONDS`, hidden for as long, and shown
/// again whenever its revision changes.
#[derive(Default)]
pub(super) struct CaretBlink {
    /// The caret revision last seen, and the clock when it changed.
    since: Option<(u64, f64)>,
}

impl CaretBlink {
    /// Whether the caret at `revision` shows at clock `now`, in seconds.
    pub(super) fn shown(&mut self, revision: u64, now: f64) -> bool {
        let since = match self.since {
            Some((seen, since)) if seen == revision => since,
            _ => {
                self.since = Some((revision, now));
                now
            }
        };
        (((now - since).max(0.0) / json_ui::CARET_BLINK_SECONDS) as u64).is_multiple_of(2)
    }
}

impl UiPresentationRuntime {
    /// Window-logical IME anchor below the last painted text field.
    pub fn menu_ime_position(&self, field: MenuField) -> Option<[f32; 2]> {
        self.screens
            .menu_caret
            .spots
            .iter()
            .rev()
            .find(|spot| spot.field == field)
            .map(|spot| [spot.left, spot.bounds.max().y()])
    }

    /// Starts a menu frame: the view's blink phase from this frame's clock, and no
    /// text drawn yet.
    pub(super) fn begin_menu_caret(&mut self, view: &mut MenuView) {
        let now = self.menu_seconds;
        let state = &mut self.screens.menu_caret;
        state.spots.clear();
        let shown = state.blink.shown(view.caret.revision, now);
        if view.field.is_some() {
            view.caret.shown = shown;
        }
    }

    pub(super) fn add_menu_text_spots(
        &mut self,
        spots: impl IntoIterator<Item = TextSpot<MenuField>>,
    ) {
        self.screens.menu_caret.spots.extend(spots);
    }

    /// The byte of `text` a press at `point` inside `field` puts its caret at,
    /// measured as the last menu frame drew the box's text.
    pub fn menu_caret_at(&mut self, point: UiPoint, field: MenuField, text: &str) -> Option<usize> {
        let spot = self
            .screens
            .menu_caret
            .spots
            .iter()
            .find(|spot| spot.field == field && spot.bounds.contains(point))?;
        let (layouts, font) = (&mut self.layouts, &self.font);
        Some(nearest_boundary(text, point.x() - spot.left, |prefix| {
            spot.width(layouts, font, prefix)
        }))
    }
}
