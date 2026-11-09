//! Removes and restores the event loop's Windows raw-input mouse registration.
use super::super::raw_mouse::RawMouseReports;
use bevy::prelude::warn;
use std::{mem::size_of, ptr::null_mut};
use windows_sys::Win32::UI::Input::{
    GetRegisteredRawInputDevices, RAWINPUTDEVICE, RIDEV_REMOVE, RegisterRawInputDevices,
};

/// HID usage page for generic desktop controls.
const GENERIC_DESKTOP_PAGE: u16 = 0x01;
/// HID generic desktop usage for a mouse.
const MOUSE_USAGE: u16 = 0x02;

/// Holds the event loop's mouse registration while it is removed, so restoring it keeps the
/// event loop's target window and flags.
#[derive(Default)]
pub(in crate::camera::focus) struct WindowsRawMouse {
    removed: Option<RAWINPUTDEVICE>,
}

impl RawMouseReports for WindowsRawMouse {
    fn set_delivery(&mut self, deliver: bool) {
        if deliver {
            let Some(registration) = self.removed.take() else {
                return;
            };
            if !register(registration) {
                warn!(
                    "could not restore raw mouse input: {}",
                    std::io::Error::last_os_error()
                );
            }
            return;
        }
        // Nothing to remove when the event loop never registered the mouse.
        let Some(registration) = registered_mouse() else {
            return;
        };
        let removal = RAWINPUTDEVICE {
            usUsagePage: GENERIC_DESKTOP_PAGE,
            usUsage: MOUSE_USAGE,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: null_mut(),
        };
        if register(removal) {
            self.removed = Some(registration);
        } else {
            warn!(
                "could not pause raw mouse input: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

/// Applies one raw input registration change for this process.
fn register(device: RAWINPUTDEVICE) -> bool {
    unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

/// Returns this process's current mouse registration, if it has one.
fn registered_mouse() -> Option<RAWINPUTDEVICE> {
    let size = size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0;
    // A null buffer reports the number of registrations.
    unsafe { GetRegisteredRawInputDevices(null_mut(), &mut count, size) };
    if count == 0 {
        return None;
    }
    let empty = RAWINPUTDEVICE {
        usUsagePage: 0,
        usUsage: 0,
        dwFlags: 0,
        hwndTarget: null_mut(),
    };
    let mut devices = vec![empty; count as usize];
    let written = unsafe { GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut count, size) };
    if written == u32::MAX {
        return None;
    }
    devices.truncate(written as usize);
    devices
        .into_iter()
        .find(|device| device.usUsagePage == GENERIC_DESKTOP_PAGE && device.usUsage == MOUSE_USAGE)
}
