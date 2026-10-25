//! Retained mod touch buttons and the taps queued for their owner's next frame callback.
use super::{Instance, MAX_IMPORT_WRITES, State, cinnabar::extension::touch as wit};
use anyhow::{Result, bail};
use ui::mod_touch::{MAX_QUEUED_TOUCH_PRESSES, TouchButton};

/// Source of per-instance owner tokens, distinct for every component instance in the process.
static NEXT_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) struct TouchState {
    /// Identifies this component instance to the host, so a tap is never delivered to a
    /// replacement instance or another mod presenting the same button id.
    owner: u64,
    pub committed: Vec<TouchButton>,
    pending: Option<Vec<TouchButton>>,
    writes: u32,
    reads: u32,
    /// Completed taps, kept across deferred frames until a frame callback runs.
    presses: Vec<String>,
    /// Taps are readable only while a frame callback runs.
    in_frame: bool,
    /// Whether the last executed frame callback received taps.
    delivered: bool,
}

impl Default for TouchState {
    fn default() -> Self {
        Self {
            owner: NEXT_OWNER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            committed: Vec::new(),
            pending: None,
            writes: 0,
            reads: 0,
            presses: Vec::new(),
            in_frame: false,
            delivered: false,
        }
    }
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
    /// Drops buttons and taps while keeping this instance's owner token.
    pub fn revoke(&mut self) {
        *self = Self {
            owner: self.owner,
            ..Self::default()
        };
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
        // Recorded before the callback, which may remove the tapped button.
        self.delivered = !self.presses.is_empty();
    }
    /// Consumes delivered taps unless the frame was deferred.
    pub fn finish_frame(&mut self, deferred: bool) {
        self.in_frame = false;
        if deferred {
            self.delivered = false;
        } else {
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
fn anchor(anchor: wit::Anchor) -> ui::mod_hud::Anchor {
    use ui::mod_hud::Anchor;
    match anchor {
        wit::Anchor::TopLeft => Anchor::TopLeft,
        wit::Anchor::TopCenter => Anchor::TopCenter,
        wit::Anchor::TopRight => Anchor::TopRight,
        wit::Anchor::Left => Anchor::Left,
        wit::Anchor::Center => Anchor::Center,
        wit::Anchor::Right => Anchor::Right,
        wit::Anchor::BottomLeft => Anchor::BottomLeft,
        wit::Anchor::BottomCenter => Anchor::BottomCenter,
        wit::Anchor::BottomRight => Anchor::BottomRight,
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

    /// This instance's owner token for presented buttons and their taps.
    pub(crate) fn touch_owner(&self) -> u64 {
        self.store.data().touch.owner
    }

    /// Whether the last executed frame callback received taps, even ones deferred earlier.
    pub(crate) fn touch_presses_delivered(&self) -> bool {
        self.is_active() && self.store.data().touch.delivered
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
