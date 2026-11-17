//! The presentation choices each platform starts with, shared by settings and the runtime.

use crate::{FrameRateLimit, VsyncMode};

/// Whether this build targets a phone or tablet platform.
pub const MOBILE_PLATFORM: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// The VSync and frame-rate choices in force until the player changes them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationDefaults {
    pub vsync: VsyncMode,
    pub frame_rate_limit: FrameRateLimit,
}

impl PresentationDefaults {
    /// The defaults of the platform this build targets.
    pub const PLATFORM: Self = Self::for_platform(MOBILE_PLATFORM);

    /// Phones and tablets synchronize to the display and add no cap of their own, so they run
    /// at whatever refresh rate the panel has. Desktop starts unsynchronized and unlimited.
    #[must_use]
    pub const fn for_platform(mobile: bool) -> Self {
        Self {
            vsync: if mobile {
                VsyncMode::On
            } else {
                VsyncMode::Off
            },
            frame_rate_limit: FrameRateLimit::Unlimited,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phones_start_synchronized_to_the_display_without_a_fixed_cap() {
        let mobile = PresentationDefaults::for_platform(true);
        assert_eq!(mobile.vsync, VsyncMode::On);
        assert_eq!(mobile.frame_rate_limit, FrameRateLimit::Unlimited);
    }

    #[test]
    fn desktop_starts_unsynchronized_and_unlimited() {
        let desktop = PresentationDefaults::for_platform(false);
        assert_eq!(desktop.vsync, VsyncMode::Off);
        assert_eq!(desktop.frame_rate_limit, FrameRateLimit::Unlimited);
    }

    /// Settings and the runtime read the type defaults, which must stay this platform's.
    #[test]
    fn the_type_defaults_are_the_platform_defaults() {
        let platform = PresentationDefaults::for_platform(MOBILE_PLATFORM);
        assert_eq!(PresentationDefaults::PLATFORM, platform);
        assert_eq!(VsyncMode::default(), platform.vsync);
        assert_eq!(VsyncMode::DEFAULT, platform.vsync);
        assert_eq!(FrameRateLimit::default(), platform.frame_rate_limit);
    }
}
