use super::raw_mouse::{RawMouseReports, install};
use bevy::{
    input::{
        InputPlugin,
        mouse::{AccumulatedMouseMotion, MouseMotion},
    },
    prelude::*,
};
use client_state::ScreenFocus;
use std::{cell::Cell, rc::Rc};

/// An 8 kHz mouse, the highest polling rate reported against menu stalls.
const REPORTS_PER_SECOND: usize = 8_000;
const FRAMES_PER_SECOND: usize = 60;
const DELTA: Vec2 = Vec2::new(0.5, -0.25);

/// Stands in for the OS: counts subscription changes and delivers reports only while subscribed.
#[derive(Clone)]
struct FakeOs {
    delivering: Rc<Cell<bool>>,
    changes: Rc<Cell<usize>>,
}

impl FakeOs {
    /// Starts subscribed, as the event loop registers raw mouse input at startup.
    fn new() -> Self {
        Self {
            delivering: Rc::new(Cell::new(true)),
            changes: Rc::new(Cell::new(0)),
        }
    }
}

impl RawMouseReports for FakeOs {
    fn set_delivery(&mut self, deliver: bool) {
        self.delivering.set(deliver);
        self.changes.set(self.changes.get() + 1);
    }
}

#[derive(Resource, Default)]
struct Frames(usize);

/// Counts completed frame updates.
fn count_frame(mut frames: ResMut<Frames>) {
    frames.0 += 1;
}

/// Reports a mouse polling at [`REPORTS_PER_SECOND`] sends during `frame`.
fn reports_in(frame: usize) -> usize {
    (frame + 1) * REPORTS_PER_SECOND / FRAMES_PER_SECOND
        - frame * REPORTS_PER_SECOND / FRAMES_PER_SECOND
}

/// The shell's published screen focus with the menu shown or hidden.
fn menu_focus(menu_visible: bool) -> ScreenFocus {
    ScreenFocus {
        menu_visible,
        ..ScreenFocus::default()
    }
}

/// Builds the input stack with the raw-mouse policy and a visible menu.
fn flood_app(os: &FakeOs) -> App {
    let mut app = App::new();
    app.add_plugins(InputPlugin)
        .insert_resource(menu_focus(true))
        .init_resource::<Frames>()
        .add_systems(Update, count_frame);
    install(&mut app, os.clone());
    app
}

/// Runs one frame of mouse movement; returns the reports the event loop had to handle.
fn flood_frame(app: &mut App, os: &FakeOs, frame: usize) -> usize {
    let delivered = if os.delivering.get() {
        reports_in(frame)
    } else {
        0
    };
    for _ in 0..delivered {
        app.world_mut().write_message(MouseMotion { delta: DELTA });
    }
    app.update();
    delivered
}

#[test]
fn menu_frames_handle_no_raw_reports_from_an_8khz_mouse() {
    let os = FakeOs::new();
    let mut app = flood_app(&os);
    // The frame that shows the menu still drains reports queued before it stopped them.
    assert_eq!(flood_frame(&mut app, &os, 0), reports_in(0));
    let mut handled = 0;
    for frame in 1..FRAMES_PER_SECOND {
        handled += flood_frame(&mut app, &os, frame);
    }
    assert_eq!(handled, 0, "raw reports handled behind the menu");
    assert_eq!(
        os.changes.get(),
        1,
        "steady menu frames must not call the OS"
    );
    assert_eq!(app.world().resource::<Frames>().0, FRAMES_PER_SECOND);
}

#[test]
fn gameplay_receives_every_report_once_the_menu_closes() {
    let os = FakeOs::new();
    let mut app = flood_app(&os);
    flood_frame(&mut app, &os, 0);
    app.insert_resource(menu_focus(false));
    // Delivery resumes at the end of the frame that closes the menu.
    assert_eq!(flood_frame(&mut app, &os, 1), 0);
    for frame in 2..FRAMES_PER_SECOND {
        let reports = flood_frame(&mut app, &os, frame);
        assert_eq!(reports, reports_in(frame));
        assert_eq!(
            app.world().resource::<AccumulatedMouseMotion>().delta,
            DELTA * reports as f32,
            "camera motion must include every report of frame {frame}"
        );
    }
    assert_eq!(os.changes.get(), 2);

    // Reopening a menu, such as pause, stops delivery again.
    app.insert_resource(menu_focus(true));
    flood_frame(&mut app, &os, 0);
    assert_eq!(flood_frame(&mut app, &os, 1), 0);
    assert_eq!(os.changes.get(), 3);
}
