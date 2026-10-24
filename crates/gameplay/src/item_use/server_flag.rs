//! Item use reconciled with the server-written `UsingItem` flag.
//!
//! Vanilla's start-or-stop-using-item system runs in each movement tick and
//! compares the actor flag with the item in use: a set flag with nothing in
//! use starts the selected item, which also reports `StartUsingItem`, and a
//! clear flag stops the use without a release transaction. Between movement
//! ticks the use is untouched, so a clear and set before the next tick leave
//! it running, and a button-up or focus change in between still releases it.

use super::*;

/// A server write waits for its tick, then for any earlier input that could replace it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerUseState {
    #[default]
    Idle,
    Start,
    Stop,
    /// The movement tick reached this stop, but an earlier release still has priority.
    StopAfterInput,
}

impl ItemUseRuntime {
    /// Notes the use button outside tick-gated maintenance: holding it lets
    /// button-up release a use the server started.
    pub fn observe_use_button(&mut self, held: bool) {
        self.button_held = held;
        if held && let Some(active) = self.active.as_mut() {
            active.button_owned = true;
        }
    }

    /// Projects queued buttons without committing their ownership or release.
    /// A projected release protects a server stop only until these edges finish.
    pub fn observe_ordered_use_buttons(
        &mut self,
        held: bool,
        later: impl IntoIterator<Item = bool>,
    ) {
        self.observe_use_button(held);
        let mut owned = self
            .active
            .as_ref()
            .is_some_and(|active| active.button_owned);
        self.deferred_button_release = owned && !held;
        for held in later {
            if self.deferred_button_release {
                break;
            }
            owned |= held && self.active.is_some();
            self.deferred_button_release = owned && !held;
        }
    }

    /// Finishes or expires the queued edges; an unfulfilled projection cannot erase a stop.
    /// A release already awaiting transaction admission remains owned until it succeeds.
    pub fn finish_ordered_use_buttons(&mut self) {
        self.deferred_button_release = false;
        if self.server_using == ServerUseState::StopAfterInput && !self.release_pending {
            self.apply_server_use_stop();
        }
    }

    /// Applies a ticked server stop without inventing a local flag write or transaction.
    fn apply_server_use_stop(&mut self) {
        self.server_using = ServerUseState::Idle;
        self.active = None;
        self.riptide_release = None;
    }

    /// Takes the item-use flag the latest local start (`true`) or end
    /// (`false`) wrote, for movement's live flag word.
    pub fn take_local_using_item(&mut self) -> Option<bool> {
        self.local_using.take()
    }

    /// Whether a server write still awaits its movement tick or earlier input.
    pub const fn has_server_using_item(&self) -> bool {
        !matches!(self.server_using, ServerUseState::Idle)
    }

    /// Reconciles a pending server `UsingItem` value with the item in use at
    /// the movement tick `frame` names, just before it simulates; returns
    /// whether a use started. A start waits while the selected stack is
    /// unverified, keeps waiting on an empty hand, and gives an item without a
    /// held use a zero-duration use.
    pub fn resolve_server_using_item(&mut self, frame: &UseFrame) -> bool {
        if self.server_using == ServerUseState::Idle {
            return false;
        }
        if matches!(
            self.server_using,
            ServerUseState::Stop | ServerUseState::StopAfterInput
        ) {
            // A release observed already, or in this frame's input, ends the use
            // with its transaction, as vanilla's input precedes its tick.
            let releasing = self.deferred_button_release
                || self.release_pending
                || self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.button_owned && !self.button_held);
            if releasing {
                self.server_using = ServerUseState::StopAfterInput;
            } else {
                self.apply_server_use_stop();
            }
            return false;
        }
        if self.active.is_some() {
            self.server_using = ServerUseState::Idle;
            return false;
        }
        // Vanilla's start ignores an empty stack, so the flag stays set and
        // every movement tick retries it; an unverified stack waits. The
        // displayed stack counts, so a predicted-empty slot also waits.
        let Some(selection) = self.displayed_selection(frame) else {
            return false;
        };
        self.server_using = ServerUseState::Idle;
        let air_use = self.crossbows.air_use(frame);
        // Keep the item's use duration even when its air action is instant.
        // Items without a use duration complete on the next normal tick.
        let (max_ticks, slowdown) = match air_use {
            Some(AirUse::Hold {
                max_ticks,
                slowdown,
                ..
            }) => (max_ticks, slowdown),
            Some(AirUse::Instant { unloaded }) => (unloaded.max_ticks, unloaded.slowdown),
            _ => (0, assets::item_use::DEFAULT_USE_MOVEMENT_MODIFIER),
        };
        self.active = Some(ActiveUse {
            selection,
            started_tick: frame.tick,
            max_ticks,
            slowdown,
            crossbow: crossbow::is_crossbow(air_use),
            riptide: frame.riptide.is_some_and(|riptide| riptide.level > 0),
            last_progress_tick: None,
            // A button already down owns the use, so its button-up releases it.
            button_owned: self.button_held,
        });
        // Held-use maintenance resumes once this tick completes.
        self.last_frame_tick = Some(frame.tick);
        true
    }

    /// Notes the session and dimension; a change drops a pending server
    /// value from the world being left.
    pub fn observe_world(&mut self, session: SessionGeneration, dimension: i32) {
        let world = Some((session, dimension));
        if self.world.is_some() && self.world != world {
            self.server_using = ServerUseState::Idle;
            self.deferred_button_release = false;
        }
        self.world = world;
    }

    /// Stops an old use after actor replacement or a spectator change, before later flags.
    /// Movement already applied the flag change, so this publishes no local write.
    pub fn stop_for_server_control(&mut self) {
        self.stop_without_release();
        // Movement already cleared the flag in packet order; a local write
        // here would overwrite a later server start's flag.
        self.local_using = None;
    }

    /// Notes whether gameplay has input focus. Vanilla's focus and chat
    /// handlers release an item in use when focus leaves gameplay, whoever
    /// started the use, so losing focus lets button-up release it.
    pub fn observe_gameplay_focus(&mut self, focused: bool) {
        if self.gameplay_focused
            && !focused
            && let Some(active) = self.active.as_mut()
        {
            active.button_owned = true;
        }
        self.gameplay_focused = focused;
    }
}
