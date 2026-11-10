//! Held dials (compasses and clocks) draw their current strip frame.

use super::{EquipmentRuntime, WornItem};

impl EquipmentRuntime {
    /// Sets the strip frames held dials show from now on.
    pub fn set_dial_frames(&mut self, frames: inventory::DialFrames) {
        self.dial_frames = frames;
    }

    /// The session icon identity a held `item` draws: the current frame of a pack's own complete
    /// strip when it carries one, else the item's own identity.
    pub(super) fn session_icon_key<'a>(&self, item: &'a WornItem) -> (&'a str, u32) {
        if let Some(strip) = assets::icon::dial::dial_strip(&item.identifier)
            && let Some(frame) = self.dial_frames.frame(strip.kind)
            && frame < inventory::strip_frames(strip.kind)
            && self
                .session_has_variants(strip.frames_identifier, inventory::strip_frames(strip.kind))
        {
            return (strip.frames_identifier, frame);
        }
        (&item.identifier, item.metadata)
    }

    /// The vanilla icon sprite a held `item` draws: a dial's current strip frame when its
    /// frames are carried, else the item's own icon.
    pub(super) fn held_icon_index(&self, item: &WornItem) -> Option<usize> {
        let (identifier, metadata) = self
            .dial_frames
            .icon_key(&item.identifier, &self.icons)
            .unwrap_or((&item.identifier, item.metadata));
        self.icons.lookup_held_index(identifier, metadata)
    }
}
