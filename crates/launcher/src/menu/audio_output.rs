//! The toast that tells the player their sound output could not be opened.

/// The toast's title.
pub const UNAVAILABLE_TITLE: &str = "No audio";

/// The toast's message; the log names the backend error.
pub const UNAVAILABLE_MESSAGE: &str = concat!(
    product_name!(),
    " can't open your sound output. It keeps retrying; the log has details."
);

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
    fn the_message_names_the_product() {
        assert!(UNAVAILABLE_MESSAGE.starts_with(crate::PRODUCT_NAME));
    }
}
