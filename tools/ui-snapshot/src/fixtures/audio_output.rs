//! The toast raised when the sound output stays unopenable.

use launcher::menu::audio_output::{UNAVAILABLE_MESSAGE, UNAVAILABLE_TITLE};

use super::{Fixture, GUI_SCALE, SIZE, State};

fn unavailable() -> Fixture {
    Fixture::Toast {
        title: UNAVAILABLE_TITLE.to_owned(),
        message: UNAVAILABLE_MESSAGE.to_owned(),
    }
}

pub(super) const STATES: &[State] = &[State {
    name: "toast-audio-output-unavailable",
    size: SIZE,
    gui_scale: GUI_SCALE,
    fixture: unavailable,
}];
