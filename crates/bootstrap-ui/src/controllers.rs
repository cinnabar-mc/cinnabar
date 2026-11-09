//! Controller button transitions for the setup and crash windows.

/// A button transition or loss of a controller's held input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    ButtonPressed(gilrs::Button),
    ButtonReleased(gilrs::Button),
    Disconnected,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Controllers;

/// The platform controller backend, polled by the window's event loop.
#[cfg(not(target_os = "macos"))]
pub struct Controllers(gilrs::Gilrs);

#[cfg(not(target_os = "macos"))]
impl Controllers {
    /// Opens the device backend, logging an initialization failure.
    pub fn new() -> Option<Self> {
        gilrs::Gilrs::new()
            .map(Self)
            .map_err(|error| tracing::warn!("bootstrap controller input unavailable: {error}"))
            .ok()
    }

    /// Returns the next button transition, skipping axis and held-button repeat events.
    pub fn next_event(&mut self) -> Option<Event> {
        while let Some(event) = self.0.next_event() {
            if let Some(event) = from_gilrs(event.event) {
                return Some(event);
            }
        }
        None
    }
}

/// Discards backend data that cannot drive the bootstrap windows.
fn from_gilrs(event: gilrs::EventType) -> Option<Event> {
    match event {
        gilrs::EventType::ButtonPressed(button, _) => Some(Event::ButtonPressed(button)),
        gilrs::EventType::ButtonReleased(button, _) => Some(Event::ButtonReleased(button)),
        gilrs::EventType::Disconnected => Some(Event::Disconnected),
        _ => None,
    }
}
