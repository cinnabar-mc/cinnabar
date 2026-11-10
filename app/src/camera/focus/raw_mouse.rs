//! Stops raw relative-mouse reports while a menu owns the visible cursor.
//!
//! A high-polling mouse sends one raw report per poll, up to 8,000 a second. Menus read only
//! the cursor position, which they sample once per frame, so those reports are pure event-loop
//! work there. On Windows every queued report is its own message, and the cursor moves between
//! them, so the OS cannot merge cursor moves into one per frame either. The loop then handles
//! thousands of messages before it can present the next frame. Gameplay locks the cursor and
//! reads the reports for camera motion, so delivery resumes as soon as no menu is showing.
use bevy::prelude::{App, NonSendMut, PostUpdate, Res};
use client_state::ScreenFocus;

/// Platform control over raw relative-mouse reports reaching the event loop.
pub(super) trait RawMouseReports {
    /// Starts (`true`) or stops (`false`) delivery of raw mouse reports.
    fn set_delivery(&mut self, deliver: bool);
}

/// Raw report delivery and the platform control that changes it.
pub(super) struct RawMouseDelivery {
    reports: Box<dyn RawMouseReports>,
    delivering: bool,
}

impl RawMouseDelivery {
    /// Starts from the event loop's default, which delivers raw reports.
    fn new(reports: impl RawMouseReports + 'static) -> Self {
        Self {
            reports: Box::new(reports),
            delivering: true,
        }
    }

    /// Calls the platform only on transitions, so steady frames make no OS calls.
    fn deliver(&mut self, deliver: bool) {
        if deliver != self.delivering {
            self.reports.set_delivery(deliver);
            self.delivering = deliver;
        }
    }
}

/// Raw reports are needed unless a menu is showing; menus use the cursor position instead.
fn wants_raw_mouse(focus: Option<&ScreenFocus>) -> bool {
    !focus.is_some_and(|focus| focus.menu_visible)
}

/// Applies the shell's final menu visibility for this frame.
fn sync_raw_mouse(focus: Option<Res<ScreenFocus>>, mut delivery: NonSendMut<RawMouseDelivery>) {
    delivery.deliver(wants_raw_mouse(focus.as_deref()));
}

/// Installs the delivery policy on the event-loop thread, which owns the raw input target.
pub(super) fn install(app: &mut App, reports: impl RawMouseReports + 'static) {
    app.insert_non_send(RawMouseDelivery::new(reports))
        .add_systems(PostUpdate, sync_raw_mouse);
}
