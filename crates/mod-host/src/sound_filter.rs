//! Granted synchronous sound-event callbacks.
use crate::{ModHost, SoundEvent};
impl ModHost {
    /// Whether this running component may inspect new sound playback requests.
    pub fn has_sound_filter(&self) -> bool {
        self.instance.has_sound_filter()
    }
    /// Returns one play/cancel decision per observation; failures leave audio to the caller.
    pub fn filter_sounds(&mut self, events: Vec<SoundEvent>) -> anyhow::Result<Vec<bool>> {
        self.instance.filter_sounds(events)
    }
}
