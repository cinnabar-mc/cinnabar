//! Mod-registered buttons: their layout nodes, stable placement names and completed taps.
use super::*;
use ui::mod_hud::Anchor;
use ui::mod_touch::{MAX_PRESENTED_TOUCH_BUTTONS, MAX_QUEUED_TOUCH_PRESSES, TouchButton};

/// Layout names carry the presented slot, so a painted button resolves to its hit id alone.
const SLOT_PREFIX: &str = "mod_";

/// The presented slot named by a layout control name.
pub(super) fn slot(name: &str) -> Option<usize> {
    let digits = name.strip_prefix(SLOT_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse()
        .ok()
        .filter(|slot| *slot < MAX_PRESENTED_TOUCH_BUTTONS)
}

/// The saved-layout and editor name of a painted control: an authored control keeps its own
/// name, and a mod button uses its stable id rather than its slot.
pub(super) fn placement_name(buttons: &[TouchButton], name: &str) -> Option<String> {
    match slot(name) {
        Some(slot) => buttons
            .get(slot)
            .map(|button| ui::mod_touch::placement_name(&button.id)),
        None => Some(name.to_owned()),
    }
}

/// Appends one `cinnabar_touch.mod_button` per presented button at its default placement.
pub(super) fn append_nodes(controls: &mut Vec<serde_json::Value>, buttons: &[TouchButton]) {
    for (slot, button) in buttons.iter().enumerate() {
        let anchor = editor::factory::json_name(button.anchor);
        let right = matches!(
            button.anchor,
            Anchor::TopRight | Anchor::Right | Anchor::BottomRight
        );
        let bottom = matches!(
            button.anchor,
            Anchor::BottomLeft | Anchor::BottomCenter | Anchor::BottomRight
        );
        // Offsets point inward from anchored edges, as HUD placements do.
        let offset = [
            if right {
                -button.offset[0]
            } else {
                button.offset[0]
            },
            if bottom {
                -button.offset[1]
            } else {
                button.offset[1]
            },
        ];
        controls.push(serde_json::json!({
            format!("{SLOT_PREFIX}{slot}@cinnabar_touch.mod_button"): {
                "anchor_from": anchor,
                "anchor_to": anchor,
                "offset": offset,
                "size": button.size,
                "$control": format!("cinnabar.touch.{SLOT_PREFIX}{slot}"),
                "$text": button.label,
                "$held_binding": format!("#touch_{SLOT_PREFIX}{slot}_pressed"),
            }
        }));
    }
}

impl TouchControls {
    /// Installs a new button set, rebuilding the layout only when it changed.
    pub(super) fn install_mod_buttons<'a>(
        &mut self,
        buttons: impl Iterator<Item = &'a TouchButton> + Clone,
    ) {
        let buttons = buttons.take(MAX_PRESENTED_TOUCH_BUTTONS);
        if self.mod_buttons.iter().eq(buttons.clone()) {
            return;
        }
        self.mod_buttons = buttons.cloned().collect();
        self.catalog = None;
        self.base = None;
        self.base_viewport = None;
        self.screen = CachedScreen::default();
    }
}

impl UiPresentationRuntime {
    /// Presents mod buttons in load order. While the layout editor is open its controls stay
    /// fixed, and the latest set is installed when it closes.
    pub fn set_mod_touch_buttons<'a>(
        &mut self,
        buttons: impl Iterator<Item = &'a TouchButton> + Clone,
    ) {
        let state = &mut self.screens.touch;
        if state.editor.is_some() {
            state.deferred_mod_buttons =
                Some(buttons.take(MAX_PRESENTED_TOUCH_BUTTONS).cloned().collect());
        } else {
            state.install_mod_buttons(buttons);
        }
    }

    /// Records a completed tap on a presented mod button; later taps beyond the bound drop.
    pub fn complete_mod_touch_tap(&mut self, hit_id: u16) {
        let state = &mut self.screens.touch;
        if let Some(button) = touch::mod_button_slot(hit_id).and_then(|s| state.mod_buttons.get(s))
            && state.mod_taps.len() < MAX_QUEUED_TOUCH_PRESSES
        {
            state.mod_taps.push(button.id.clone());
        }
    }

    /// Holds a presented button with a stationary finger, for captures of its pressed art.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_mod_touch_button(&mut self, slot: usize) {
        self.screens.touch.contacts = vec![TouchContact {
            contact_id: slot as u64 + 1,
            activity_sequence: 1,
            position: [0.; 2],
            delta: [0.; 2],
            hit_id: touch::mod_button(slot),
        }];
    }

    /// Button ids tapped since the last call, oldest first.
    pub fn take_mod_touch_taps(&mut self) -> Vec<String> {
        std::mem::take(&mut self.screens.touch.mod_taps)
    }
}
