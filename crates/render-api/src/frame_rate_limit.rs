//! The player's frame-rate limit, shared by settings, launcher persistence and presentation.

use std::num::NonZeroU16;

/// Whether this build targets a phone or tablet platform.
pub const MOBILE_PLATFORM: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// Phones and tablets start gameplay capped at this rate and never draw menus faster, which
/// keeps sustained clocks out of deep thermal throttling.
pub const MOBILE_FRAME_RATE_CAP: NonZeroU16 = NonZeroU16::new(60).unwrap();

/// The player's FPS cap; presentation and variable refresh may impose a lower rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FrameRateLimit {
    /// Frames per second.
    Fixed(NonZeroU16),
    Unlimited,
}

impl FrameRateLimit {
    /// The limit this platform starts with until the player chooses one.
    pub const DEFAULT: Self = Self::platform_default(MOBILE_PLATFORM);

    /// The starting limit for a mobile or desktop platform: the mobile cap or Unlimited.
    #[must_use]
    pub const fn platform_default(mobile: bool) -> Self {
        if mobile {
            Self::Fixed(MOBILE_FRAME_RATE_CAP)
        } else {
            Self::Unlimited
        }
    }

    /// The highest menu rate a mobile or desktop platform allows whatever the player's limit;
    /// `None` leaves menus at the display refresh.
    #[must_use]
    pub const fn platform_menu_ceiling(mobile: bool) -> Option<NonZeroU16> {
        if mobile {
            Some(MOBILE_FRAME_RATE_CAP)
        } else {
            None
        }
    }
}

impl Default for FrameRateLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_starts_at_the_mobile_cap_and_desktop_unlimited() {
        assert_eq!(
            FrameRateLimit::platform_default(true),
            FrameRateLimit::Fixed(NonZeroU16::new(60).unwrap())
        );
        assert_eq!(
            FrameRateLimit::platform_default(false),
            FrameRateLimit::Unlimited
        );
        assert_eq!(
            FrameRateLimit::default(),
            FrameRateLimit::platform_default(MOBILE_PLATFORM)
        );
    }

    #[test]
    fn only_mobile_caps_menus() {
        assert_eq!(
            FrameRateLimit::platform_menu_ceiling(true),
            NonZeroU16::new(60)
        );
        assert_eq!(FrameRateLimit::platform_menu_ceiling(false), None);
    }
}
