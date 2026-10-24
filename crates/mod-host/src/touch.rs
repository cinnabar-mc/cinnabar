//! Retained mod touch buttons and the taps queued for their owner's next frame callback.
use super::{
    Instance, MAX_IMPORT_WRITES, State, cinnabar::extension::hud, cinnabar::extension::touch as wit,
};
use anyhow::{Result, bail};
use ui::mod_touch::{MAX_QUEUED_TOUCH_PRESSES, TouchButton};

#[derive(Default)]
pub(super) struct TouchState {
    pub committed: Vec<TouchButton>,
    pending: Option<Vec<TouchButton>>,
    writes: u32,
    reads: u32,
    /// Completed taps, kept across deferred frames until a frame callback runs.
    presses: Vec<String>,
    /// Taps are readable only while a frame callback runs.
    in_frame: bool,
}

impl TouchState {
    /// Starts one callback's transaction and import budget.
    pub fn begin_callback(&mut self) {
        self.pending = None;
        self.writes = 0;
        self.reads = 0;
    }
    /// Publishes a successful callback's staged set; dropped taps of removed buttons expire.
    pub fn commit(&mut self) {
        if let Some(buttons) = self.pending.take() {
            self.presses
                .retain(|id| buttons.iter().any(|button| &button.id == id));
            self.committed = buttons;
        }
    }
    pub fn revoke(&mut self) {
        *self = Self::default();
    }
    /// Admits a tap on a committed button while the bounded queue has room.
    pub fn queue(&mut self, id: &str) -> bool {
        let accepted = self.presses.len() < MAX_QUEUED_TOUCH_PRESSES
            && self.committed.iter().any(|button| button.id == id);
        if accepted {
            self.presses.push(id.to_owned());
        }
        accepted
    }
    /// Exposes queued taps to the frame callback about to run.
    pub fn begin_frame(&mut self) {
        self.in_frame = true;
    }
    /// Consumes delivered taps unless the frame was deferred.
    pub fn finish_frame(&mut self, deferred: bool) {
        self.in_frame = false;
        if !deferred {
            self.presses.clear();
        }
    }
}

impl wit::Host for State {
    fn set_buttons(&mut self, buttons: Vec<wit::Button>) -> Result<Result<(), String>> {
        self.touch.writes += 1;
        if self.touch.writes > MAX_IMPORT_WRITES {
            bail!("touch import budget exhausted");
        }
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        let buttons: Vec<_> = buttons
            .into_iter()
            .map(|button| TouchButton {
                id: button.id,
                label: button.label,
                anchor: anchor(button.anchor),
                offset: [button.offset.x, button.offset.y],
                size: [button.size.x, button.size.y],
            })
            .collect();
        if let Err(error) = ui::mod_touch::validate(&buttons) {
            return Ok(Err(error));
        }
        self.touch.pending = Some(buttons);
        Ok(Ok(()))
    }

    fn read_presses(&mut self) -> Result<Result<Vec<String>, String>> {
        self.touch.reads += 1;
        if self.touch.reads > MAX_IMPORT_WRITES {
            bail!("touch import budget exhausted");
        }
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        Ok(Ok(if self.touch.in_frame {
            self.touch.presses.clone()
        } else {
            Vec::new()
        }))
    }
}

/// Matches the WIT anchor exhaustively against the shared HUD anchor.
fn anchor(anchor: hud::Anchor) -> ui::mod_hud::Anchor {
    use ui::mod_hud::Anchor;
    match anchor {
        hud::Anchor::TopLeft => Anchor::TopLeft,
        hud::Anchor::TopCenter => Anchor::TopCenter,
        hud::Anchor::TopRight => Anchor::TopRight,
        hud::Anchor::Left => Anchor::Left,
        hud::Anchor::Center => Anchor::Center,
        hud::Anchor::Right => Anchor::Right,
        hud::Anchor::BottomLeft => Anchor::BottomLeft,
        hud::Anchor::BottomCenter => Anchor::BottomCenter,
        hud::Anchor::BottomRight => Anchor::BottomRight,
    }
}

impl Instance {
    /// Committed buttons of an active instance that still holds controls authority.
    pub(crate) fn touch_buttons(&self) -> &[TouchButton] {
        let state = self.store.data();
        if self.is_active() && state.grants.controls {
            &state.touch.committed
        } else {
            &[]
        }
    }

    /// Queues one completed tap for the next frame callback. Taps on a button the instance no
    /// longer retains, or beyond the bounded queue, are dropped and return false.
    pub(crate) fn queue_touch_press(&mut self, id: &str) -> bool {
        let active = self.is_active();
        let state = self.store.data_mut();
        active && state.grants.controls && state.touch.queue(id)
    }
}

#[cfg(test)]
#[path = "touch_tests.rs"]
mod tests;
