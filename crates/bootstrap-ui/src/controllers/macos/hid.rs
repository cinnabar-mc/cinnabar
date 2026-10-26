//! Preserve HID-only controllers while Apple handles its supported device classes.

use std::collections::HashSet;

use objc2::{ClassType, msg_send};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString};
use objc2_game_controller::GCController;
use objc2_io_kit::{
    IOHIDDevice, IOHIDManager, kHIDPage_GenericDesktop, kHIDUsage_GD_GamePad,
    kHIDUsage_GD_Joystick, kHIDUsage_GD_MultiAxisController, kIOHIDDeviceUsageKey,
    kIOHIDDeviceUsagePageKey, kIOHIDProductIDKey, kIOHIDVendorIDKey,
};

use super::Event;

pub(super) struct Fallback {
    raw: Option<gilrs::Gilrs>,
    manager: CFRetained<IOHIDManager>,
    routes: Routes,
    matchers: CFRetained<CFArray<CFDictionary<CFString, CFNumber>>>,
}

impl Fallback {
    /// Opens the existing HID backend and a controller-only capability query.
    pub(super) fn new() -> Self {
        let manager = IOHIDManager::new(None, 0);
        let page_key = CFString::from_static_str(kIOHIDDeviceUsagePageKey.to_str().unwrap());
        let usage_key = CFString::from_static_str(kIOHIDDeviceUsageKey.to_str().unwrap());
        let page = CFNumber::new_i32(kHIDPage_GenericDesktop as i32);
        let matchers = [
            kHIDUsage_GD_Joystick,
            kHIDUsage_GD_GamePad,
            kHIDUsage_GD_MultiAxisController,
        ]
        .map(|usage| {
            let usage = CFNumber::new_i32(usage as i32);
            CFDictionary::from_slices(&[&*page_key, &*usage_key], &[&*page, &*usage])
        });
        let matchers = CFArray::from_retained_objects(&matchers);
        // SAFETY: Each dictionary contains numeric HID usage matching criteria.
        unsafe { manager.set_device_matching_multiple(Some(matchers.as_opaque())) };
        let raw = gilrs::Gilrs::new()
            .map_err(|error| tracing::warn!("bootstrap HID controller input unavailable: {error}"))
            .ok();
        Self {
            raw,
            manager,
            routes: Routes::default(),
            matchers,
        }
    }

    /// Forwards only devices outside Apple's supported controller classes.
    pub(super) fn next_event(&mut self) -> Option<Event> {
        let raw = self.raw.as_mut()?;
        while let Some(event) = raw.next_event() {
            let id = usize::from(event.id);
            if event.event == gilrs::EventType::Connected {
                // SAFETY: Resetting these criteria refreshes enumeration for hotplugged devices
                // without opening devices or scheduling another input callback source.
                unsafe {
                    self.manager
                        .set_device_matching_multiple(Some(self.matchers.as_opaque()))
                };
                let gamepad = raw.gamepad(event.id);
                let native =
                    native_support(&self.manager, gamepad.vendor_id(), gamepad.product_id());
                self.routes.connected(id, native);
            }
            if let Some(event) =
                super::super::from_gilrs(event.event).and_then(|event| self.routes.event(id, event))
            {
                return Some(event);
            }
        }
        None
    }
}

#[derive(Default)]
struct Routes {
    native: HashSet<usize>,
}

impl Routes {
    /// Classifies each new connection, including a reused backend ID.
    fn connected(&mut self, id: usize, native: bool) {
        if native {
            self.native.insert(id);
        } else {
            self.native.remove(&id);
        }
    }

    /// Suppresses native duplicates, including any late callbacks after disconnection.
    fn event(&self, id: usize, event: Event) -> Option<Event> {
        (!self.native.contains(&id)).then_some(event)
    }
}

/// Queries the connected HID class using the same vendor/product identifiers gilrs reports.
fn native_support(manager: &IOHIDManager, vendor: Option<u16>, product: Option<u16>) -> bool {
    let (Some(vendor), Some(product), Some(devices)) = (vendor, product, manager.devices()) else {
        return false;
    };
    let mut pointers = vec![std::ptr::null(); devices.count() as usize];
    // SAFETY: IOHIDManager returns a set of retained IOHIDDevice references; the buffer has
    // exactly the set's size and the set stays alive while its members are inspected.
    unsafe { devices.values(pointers.as_mut_ptr()) };
    pointers.into_iter().any(|pointer| {
        // SAFETY: The pointer belongs to the live IOHIDManager device set above.
        let device = unsafe { &*pointer.cast::<IOHIDDevice>() };
        if device_number(device, kIOHIDVendorIDKey.to_str().unwrap()) != Some(vendor)
            || device_number(device, kIOHIDProductIDKey.to_str().unwrap()) != Some(product)
        {
            return false;
        }
        let synthetic = device
            .property(&CFString::from_static_str("GCSyntheticDevice"))
            .and_then(|value| value.downcast::<CFBoolean>().ok())
            .is_some_and(|value| value.value());
        // SAFETY: This public macOS API takes a live IOHIDDeviceRef and returns BOOL.
        // The generated bindings omit it because its argument belongs to IOKit.
        let supported: bool =
            unsafe { msg_send![GCController::class(), supportsHIDDevice: device] };
        synthetic || supported
    })
}

/// Reads an unsigned HID identifier without inventing an identity for missing properties.
fn device_number(device: &IOHIDDevice, key: &'static str) -> Option<u16> {
    let value = device.property(&CFString::from_static_str(key))?;
    u16::try_from(value.downcast_ref::<CFNumber>()?.as_i32()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_native_and_hid_only_controllers_deliver_each_transition_once() {
        let mut routes = Routes::default();
        routes.connected(0, true);
        routes.connected(1, false);
        for event in [
            Event::ButtonPressed(gilrs::Button::South),
            Event::ButtonReleased(gilrs::Button::South),
            Event::Disconnected,
        ] {
            assert_eq!(routes.event(0, event), None);
            assert_eq!(routes.event(1, event), Some(event));
        }
        assert_eq!(
            routes.event(0, Event::ButtonPressed(gilrs::Button::South)),
            None
        );
        routes.connected(0, false);
        assert_eq!(
            routes.event(0, Event::ButtonPressed(gilrs::Button::South)),
            Some(Event::ButtonPressed(gilrs::Button::South))
        );
    }
}
