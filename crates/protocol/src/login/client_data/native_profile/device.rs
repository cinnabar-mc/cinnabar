//! Host/browser metadata and compatible persisted install identifier migration.

use super::NativeDeviceProfile;
use bridge::contract::*;

impl NativeDeviceProfile {
    /// Reports this build's platform and an edge-detected model, without fabricating hardware.
    pub fn for_host(model: &str) -> Self {
        Self::new(host_os(std::env::consts::OS), model.trim())
    }

    /// Uses only browser-reported identity; an unrecognized browser retains its own claims.
    pub fn browser(agent: &str, apple_mobile: bool) -> Option<Self> {
        let os = if apple_mobile {
            NATIVE_DEVICE_OS_IOS
        } else if agent.contains("Android") {
            NATIVE_DEVICE_OS_ANDROID
        } else if ["iPhone", "iPad", "iPod"]
            .iter()
            .any(|name| agent.contains(name))
        {
            NATIVE_DEVICE_OS_IOS
        } else if agent.contains("Windows NT") {
            NATIVE_DEVICE_OS_WINDOWS
        } else if agent.contains("Macintosh") || agent.contains("Mac OS X") {
            NATIVE_DEVICE_OS_MACOS
        } else if agent.contains("Linux") || agent.contains("CrOS") {
            NATIVE_DEVICE_OS_LINUX
        } else {
            return None;
        };
        Some(Self::new(os, ""))
    }

    /// Refreshes host claims while retaining a saved identifier with the host's expected format.
    pub fn migrate(&self, saved: Option<&[u8]>) -> Self {
        let mut current = self.clone();
        if let Some(saved) = saved.and_then(|bytes| serde_json::from_slice::<Self>(bytes).ok())
            && compatible_id(self.os, &saved.id)
        {
            current.id = saved.id;
        }
        current
    }

    /// Creates metadata with a new platform-formatted identifier, independent of account state.
    fn new(os: i32, model: &str) -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        Self {
            os,
            model: model.to_owned(),
            id: if os == NATIVE_DEVICE_OS_IOS {
                id.to_ascii_uppercase()
            } else {
                id
            },
        }
    }

    /// The selected desktop and iOS categories override stale client claims; Android fills defaults.
    pub(super) fn platform(&self, default: bool) -> Option<i32> {
        match self.os {
            NATIVE_DEVICE_OS_MACOS
            | NATIVE_DEVICE_OS_WINDOWS10
            | NATIVE_DEVICE_OS_WINDOWS
            | NATIVE_DEVICE_OS_LINUX => Some(NATIVE_DEVICE_PLATFORM_DESKTOP),
            NATIVE_DEVICE_OS_IOS => Some(NATIVE_DEVICE_PLATFORM_MOBILE),
            NATIVE_DEVICE_OS_ANDROID if default => Some(NATIVE_DEVICE_PLATFORM_MOBILE),
            _ => None,
        }
    }

    /// Desktop builds use mouse input; mobile and unknown targets default to touch.
    pub(super) fn input_mode(&self) -> u32 {
        if matches!(
            self.os,
            NATIVE_DEVICE_OS_MACOS
                | NATIVE_DEVICE_OS_WINDOWS10
                | NATIVE_DEVICE_OS_WINDOWS
                | NATIVE_DEVICE_OS_LINUX
        ) {
            NATIVE_DEVICE_INPUT_MOUSE
        } else {
            NATIVE_DEVICE_INPUT_TOUCH
        }
    }
}

/// Maps the native build target to its server-facing platform.
fn host_os(os: &str) -> i32 {
    match os {
        "android" => NATIVE_DEVICE_OS_ANDROID,
        "ios" => NATIVE_DEVICE_OS_IOS,
        "macos" => NATIVE_DEVICE_OS_MACOS,
        "windows" => NATIVE_DEVICE_OS_WINDOWS,
        "linux" => NATIVE_DEVICE_OS_LINUX,
        _ => NATIVE_DEVICE_OS_UNKNOWN,
    }
}

/// Classifies compatible hexadecimal IDs, treating mixed-case IDs as upper hex.
fn compatible_id(os: i32, id: &str) -> bool {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let upper = id.bytes().any(|byte| byte.is_ascii_uppercase());
    upper == (os == NATIVE_DEVICE_OS_IOS)
}

#[cfg(test)]
mod tests;
