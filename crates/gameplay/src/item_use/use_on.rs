//! The held use an item's successful use on a block starts, as vanilla's brush does.

use super::*;

/// An admitted item use on a block whose held use waits for the next item-use step.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct UseOnStart {
    selection: FrozenMiningSelection,
    max_ticks: u32,
    slowdown: f64,
}

impl ItemUseRuntime {
    /// Queues the held use an admitted item use on a block starts, as vanilla's
    /// `BrushItem::_useOn` starts using the brush with its maximum use duration. The next
    /// step starts it unless a use is already running, since vanilla's start ignores the
    /// item it is already using. The use button owns it, so button-up sends the release.
    pub fn start_use_on(
        &mut self,
        selection: FrozenMiningSelection,
        max_ticks: u32,
        slowdown: f64,
    ) {
        self.use_on_start = Some(UseOnStart {
            selection,
            max_ticks,
            slowdown,
        });
    }

    /// Whether an item use on a block still waits to start its held use.
    pub const fn use_on_pending(&self) -> bool {
        self.use_on_start.is_some()
    }

    /// Starts a queued use on a block at the frame's tick; returns whether a use started.
    /// A selection that no longer holds the item drops it, as a reselection ends a use.
    pub(super) fn begin_use_on(&mut self, frame: &UseFrame) -> bool {
        let Some(start) = self.use_on_start.take() else {
            return false;
        };
        let reselected = frame.selection.as_ref().is_some_and(|selection| {
            selection.slot != start.selection.slot
                || selection.item.network_id() != start.selection.item.network_id()
        });
        if self.active.is_some() || reselected {
            return false;
        }
        self.active = Some(ActiveUse {
            selection: start.selection,
            started_tick: frame.tick,
            max_ticks: start.max_ticks,
            slowdown: start.slowdown,
            crossbow: false,
            riptide: false,
            last_progress_tick: None,
            button_owned: true,
        });
        self.reported_using_item = Some(true);
        true
    }
}
