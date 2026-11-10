//! The held use an item's successful use on a block starts, as vanilla's brush does.

use super::*;

impl ItemUseRuntime {
    /// Starts the held use an admitted item use on a block begins on its build tick, as
    /// vanilla's `BrushItem::_useOn` starts using the brush with its maximum use duration.
    /// A running use of the same selection is kept, since vanilla's start ignores the item it
    /// is already using; a stale use of another selection is replaced. The use button owns
    /// the use, so button-up sends the release. Returns whether a use started, so the caller
    /// can flag that tick's input.
    pub fn start_use_on(
        &mut self,
        selection: FrozenMiningSelection,
        max_ticks: u32,
        slowdown: f64,
        tick: u64,
    ) -> bool {
        if self.active.as_ref().is_some_and(|active| {
            active.selection.slot == selection.slot
                && active.selection.item.network_id() == selection.item.network_id()
        }) {
            return false;
        }
        // A replaced use ends without a release, taking its captured release state along.
        if self.active.is_some() {
            self.stop_without_release();
        }
        self.active = Some(ActiveUse {
            selection,
            started_tick: tick,
            max_ticks,
            slowdown,
            crossbow: false,
            riptide: false,
            last_progress_tick: None,
            button_owned: true,
        });
        self.reported_using_item = Some(true);
        // A local start writes the item-use flag after any server write, as a step's does.
        self.server_using = ServerUseState::Idle;
        self.deferred_button_release = false;
        self.local_using = Some(true);
        true
    }
}
