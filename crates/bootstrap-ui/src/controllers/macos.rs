//! Apple controller events do not require the optional HID compatibility layer.

use std::{
    collections::VecDeque,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

use block2::RcBlock;
use gilrs::Button;
use objc2::{MainThreadMarker, rc::Retained, runtime::Bool};
use objc2_game_controller::{GCController, GCControllerButtonInput};

use super::Event;

type Delivery = (Arc<AtomicBool>, Event);

/// Game Controller callbacks are queued until the window polls them on the main thread.
pub struct Controllers {
    devices: Vec<Device>,
    sender: Sender<Delivery>,
    events: Receiver<Delivery>,
    pending: VecDeque<Event>,
}

impl Controllers {
    /// Initializes controller discovery after the native window has been created.
    pub fn new() -> Option<Self> {
        MainThreadMarker::new()?;
        let (sender, events) = mpsc::channel();
        Some(Self {
            devices: Vec::new(),
            sender,
            events,
            pending: VecDeque::new(),
        })
    }

    /// Discovers connected controllers and drains ordered button callbacks without repeats.
    pub fn next_event(&mut self) -> Option<Event> {
        self.refresh();
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        while let Ok((active, event)) = self.events.try_recv() {
            if active.load(Ordering::Relaxed) {
                return Some(event);
            }
        }
        None
    }

    /// Retains devices by identity so enumeration order cannot swap their callbacks.
    fn refresh(&mut self) {
        // SAFETY: Called by the window's main-thread event loop; the framework owns discovery.
        let connected = unsafe { GCController::controllers() };
        let connected: Vec<_> = (0..connected.count())
            .map(|index| connected.objectAtIndex(index))
            .collect();
        self.devices.retain(|device| {
            let present = connected
                .iter()
                .any(|controller| *controller == device.controller);
            if !present {
                self.pending.push_back(Event::Disconnected);
            }
            present
        });
        for controller in connected {
            if !self
                .devices
                .iter()
                .any(|device| device.controller == controller)
                && let Some(device) = Device::new(controller, &self.sender)
            {
                self.devices.push(device);
            }
        }
    }
}

struct Device {
    controller: Retained<GCController>,
    buttons: Vec<Retained<GCControllerButtonInput>>,
    active: Arc<AtomicBool>,
}

impl Device {
    /// Subscribes to the extended profile's menu controls using Apple's semantic button names.
    fn new(controller: Retained<GCController>, sender: &Sender<Delivery>) -> Option<Self> {
        // SAFETY: These profile getters return retained framework objects and take no pointers.
        let controls = unsafe {
            let profile = controller.extendedGamepad()?;
            let dpad = profile.dpad();
            [
                (profile.buttonA(), Button::South),
                (profile.buttonB(), Button::East),
                (dpad.up(), Button::DPadUp),
                (dpad.down(), Button::DPadDown),
                (dpad.left(), Button::DPadLeft),
                (dpad.right(), Button::DPadRight),
                (profile.leftShoulder(), Button::LeftTrigger),
                (profile.rightShoulder(), Button::RightTrigger),
            ]
        };
        let active = Arc::new(AtomicBool::new(true));
        let mut buttons = Vec::with_capacity(controls.len());
        for (input, button) in controls {
            let sender = sender.clone();
            let active = Arc::clone(&active);
            let handler = RcBlock::new(
                move |_: NonNull<GCControllerButtonInput>, _: f32, pressed: Bool| {
                    let event = if pressed.as_bool() {
                        Event::ButtonPressed(button)
                    } else {
                        Event::ButtonReleased(button)
                    };
                    let _ = sender.send((Arc::clone(&active), event));
                },
            );
            // SAFETY: The framework copies the valid block. Its captures own all callback state,
            // are thread-safe, and neither retain nor dereference the supplied input pointer.
            unsafe { input.setPressedChangedHandler(RcBlock::as_ptr(&handler)) };
            buttons.push(input);
        }
        Some(Self {
            controller,
            buttons,
            active,
        })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        for button in &self.buttons {
            // SAFETY: Null removes our callback while the retained button is still alive.
            unsafe { button.setPressedChangedHandler(std::ptr::null_mut()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_button_callbacks_preserve_taps_and_retire_on_disconnect() {
        // SAFETY: A framework snapshot supplies a full extended profile without physical hardware.
        let controller = unsafe { GCController::controllerWithExtendedGamepad() };
        let (sender, events) = mpsc::channel();
        let device = Device::new(controller.clone(), &sender).unwrap();
        // SAFETY: The snapshot has the complete extended profile.
        let button = unsafe { controller.extendedGamepad().unwrap().buttonA() };
        // SAFETY: Device installed this block; the retained button and callback live for both calls.
        unsafe {
            let handler = &*button.pressedChangedHandler();
            handler.call((NonNull::from(&*button), 1.0, Bool::YES));
            handler.call((NonNull::from(&*button), 0.0, Bool::NO));
        }
        let (active, press) = events.try_recv().unwrap();
        assert_eq!(press, Event::ButtonPressed(Button::South));
        assert_eq!(
            events.try_recv().unwrap().1,
            Event::ButtonReleased(Button::South)
        );
        assert!(events.try_recv().is_err());
        assert!(active.load(Ordering::Relaxed));
        let button = button.clone();
        drop(device);
        assert!(!active.load(Ordering::Relaxed));
        // SAFETY: The test still retains the button; reading its handler takes no pointers.
        assert!(unsafe { button.pressedChangedHandler() }.is_null());
    }
}
