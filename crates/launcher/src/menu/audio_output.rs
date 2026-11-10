//! The toast that tells the player their sound output could not be opened, with the fix
//! for their platform.

/// The toast's title.
pub const UNAVAILABLE_TITLE: &str = "No audio";

/// The toast's message: the likeliest fix on this platform, short enough for the toast's
/// single line. Linux plays through ALSA's `default` device, which needs the sound
/// server's ALSA plugin.
#[cfg(target_os = "linux")]
pub const UNAVAILABLE_MESSAGE: &str = concat!(
    "Install pipewire-alsa or your ALSA PulseAudio plugin, then restart ",
    product_name!(),
    "."
);
#[cfg(windows)]
pub const UNAVAILABLE_MESSAGE: &str = concat!(
    "Pick an output device in Sound settings; ",
    product_name!(),
    " retries on its own."
);
#[cfg(target_os = "macos")]
pub const UNAVAILABLE_MESSAGE: &str = concat!(
    "Pick an output device in System Settings > Sound; ",
    product_name!(),
    " retries on its own."
);
#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub const UNAVAILABLE_MESSAGE: &str = concat!(
    product_name!(),
    " can't open your sound output and keeps retrying."
);

/// The player guide with step-by-step fixes for every platform.
pub const HELP_URL: &str = concat!(repository_url!(), "/blob/dev/docs/audio-troubleshooting.md");

/// Raises one toast per sustained output outage.
#[derive(Debug, Default)]
pub struct OutageNotice {
    shown: bool,
}

impl OutageNotice {
    /// Takes the current outage state and returns whether to raise the toast: true only
    /// when an outage begins. A recovered output re-arms the notice for the next outage.
    pub fn observe(&mut self, unavailable: bool) -> bool {
        !std::mem::replace(&mut self.shown, unavailable) && unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The longest message that fits the toast's single line at the default GUI scale.
    const MAX_MESSAGE_CHARS: usize = 80;

    #[test]
    fn each_outage_raises_one_notice() {
        let mut notice = OutageNotice::default();
        assert!(!notice.observe(false));
        assert!(notice.observe(true));
        assert!(!notice.observe(true));
        assert!(!notice.observe(true));
        assert!(!notice.observe(false));
        assert!(notice.observe(true));
    }

    #[test]
    fn the_message_names_the_product_and_fits_one_toast_line() {
        assert!(UNAVAILABLE_MESSAGE.contains(crate::PRODUCT_NAME));
        assert!(UNAVAILABLE_MESSAGE.chars().count() <= MAX_MESSAGE_CHARS);
    }

    #[test]
    fn the_help_link_points_at_the_guide_in_this_repository() {
        let guide = HELP_URL
            .strip_prefix(crate::REPOSITORY_URL)
            .and_then(|path| path.strip_prefix("/blob/dev/"))
            .expect("the guide lives on the dev branch of this repository");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(guide);
        assert!(path.is_file(), "missing {}", path.display());
    }
}
