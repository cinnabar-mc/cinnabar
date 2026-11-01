//! Deterministic frame, event and session-loading allowances shared across local instances.

use server_experience::runtime::{CALLBACK_FUEL, LOAD_FUEL};
use std::sync::{Arc, Mutex};

mod reservation;
pub(crate) use reservation::FrameReservation;

/// Expensive event and loading lanes each admit one complete callback per frame.
const LARGE_CALLBACKS_PER_FRAME: usize = 1;
const _: () = assert!(crate::FRAME_FUEL < crate::WORLD_ENTRY_FRAME_FUEL);
const _: () = assert!(crate::WORLD_ENTRY_FRAME_FUEL < CALLBACK_FUEL);
const _: () = assert!(CALLBACK_FUEL < LOAD_FUEL);

#[derive(Default)]
struct Allowance {
    frame: u64,
    generation: u64,
    debt: u64,
    events: usize,
    loads: usize,
    world_entries: usize,
}

/// Whether a guest callback ran or must be delivered in a later frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    Executed,
    Deferred,
    /// A started invocation retains its store, fuel and private transaction.
    Suspended,
}

/// Preserves each callback's full fuel limit instead of trapping on a partial frame allowance.
#[derive(Clone)]
pub struct CallbackBudget {
    allowance: Arc<Mutex<Allowance>>,
    pub(crate) time: crate::frame_time::FrameTime,
}
impl CallbackBudget {
    /// Creates the shared regular-frame allowance and bounded event/loading lanes.
    pub fn new(fuel: u64) -> Self {
        Self::with_time(fuel, Default::default())
    }

    /// Uses a caller-owned monotonic clock for deterministic scheduling boundaries.
    pub fn with_clock(
        fuel: u64,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> Self {
        Self::with_time(
            fuel,
            crate::frame_time::FrameTime::with_clock(Arc::new(clock)),
        )
    }

    /// Initializes all allowance lanes against the selected scheduling clock.
    fn with_time(fuel: u64, time: crate::frame_time::FrameTime) -> Self {
        let budget = Self {
            allowance: Arc::new(Mutex::new(Allowance::default())),
            time,
        };
        budget.reset(fuel);
        budget
    }
    /// Starts a new frame before any guest input or lifecycle callback.
    pub fn reset(&self, fuel: u64) {
        self.time.reset();
        let mut remaining = self.allowance.lock().expect("mod callback budget");
        *remaining = Allowance {
            frame: fuel.saturating_sub(remaining.debt),
            generation: remaining.generation.wrapping_add(1),
            debt: remaining.debt.saturating_sub(fuel),
            events: LARGE_CALLBACKS_PER_FRAME,
            loads: LARGE_CALLBACKS_PER_FRAME,
            world_entries: LARGE_CALLBACKS_PER_FRAME,
        };
    }
    /// Shares a soft time allowance among current runnable owners, preserving scheduling debt.
    pub fn schedule_frames(&self, owners: &[crate::FrameOwner]) {
        self.time.configure(owners);
    }

    /// Drops an instance's account immediately, even if its removed host remains alive.
    pub fn forget_frame(&self, owner: crate::FrameOwner) {
        self.time.forget(owner);
    }

    /// Reserves a full callback allowance, or defers it without reducing its guest fuel limit.
    pub(crate) fn reserve(&self, fuel: u64) -> Option<u64> {
        if fuel == crate::WORLD_ENTRY_FRAME_FUEL {
            return self.reserve_world_entry_frame();
        }
        let mut remaining = self.allowance.lock().expect("mod callback budget");
        if fuel == LOAD_FUEL {
            if remaining.loads == 0 {
                return None;
            }
            remaining.loads -= 1;
        } else if fuel == CALLBACK_FUEL {
            if remaining.events == 0 {
                return None;
            }
            remaining.events -= 1;
        } else {
            if remaining.frame < fuel {
                return None;
            }
            remaining.frame -= fuel;
        }
        Some(fuel)
    }
    /// Reserves one full world-entry allowance, borrowing against later frames as needed.
    /// Admission still needs a full ordinary allowance; refunds settle unused borrowing first.
    pub(crate) fn reserve_world_entry_frame(&self) -> Option<u64> {
        let mut remaining = self.allowance.lock().expect("mod callback budget");
        if remaining.world_entries == 0 || remaining.frame < crate::FRAME_FUEL {
            return None;
        }
        remaining.debt += crate::WORLD_ENTRY_FRAME_FUEL.saturating_sub(remaining.frame);
        remaining.frame = remaining
            .frame
            .saturating_sub(crate::WORLD_ENTRY_FRAME_FUEL);
        remaining.world_entries -= 1;
        Some(crate::WORLD_ENTRY_FRAME_FUEL)
    }
    /// Returns unused reservation fuel, paying world-entry debt before restoring capacity.
    pub(crate) fn refund(&self, fuel: u64) {
        let mut remaining = self.allowance.lock().expect("mod callback budget");
        remaining.frame += fuel.saturating_sub(remaining.debt);
        remaining.debt = remaining.debt.saturating_sub(fuel);
    }
    /// Whether the loading lane can admit another hot replacement this frame.
    pub(crate) fn can_load(&self) -> bool {
        self.allowance.lock().expect("mod callback budget").loads != 0
    }
    /// Reports regular-frame fuel remaining after completed frame callbacks.
    pub fn remaining(&self) -> u64 {
        self.allowance.lock().expect("mod callback budget").frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservations_never_turn_a_partial_frame_allowance_into_a_guest_fuel_limit() {
        let budget = CallbackBudget::new(crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(crate::FRAME_FUEL), None);
        assert_eq!(budget.remaining(), crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
        assert_eq!(budget.reserve(LOAD_FUEL), None);
        let event = server_experience::runtime::CALLBACK_FUEL;
        assert_eq!(budget.reserve(event), Some(event));
        assert_eq!(budget.reserve(event), None);
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
    }
    #[test]
    fn world_entry_lane_is_bounded_and_independent_of_events_and_loads() {
        let budget = CallbackBudget::new(crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve_world_entry_frame(), None);
        budget.refund(1);
        assert_eq!(
            budget.reserve_world_entry_frame(),
            Some(crate::WORLD_ENTRY_FRAME_FUEL)
        );
        assert_eq!(budget.remaining(), 0);
        budget.refund(crate::WORLD_ENTRY_FRAME_FUEL);
        assert_eq!(budget.reserve_world_entry_frame(), None);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
        assert_eq!(
            budget.reserve(server_experience::runtime::CALLBACK_FUEL),
            Some(server_experience::runtime::CALLBACK_FUEL)
        );
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        assert_eq!(
            budget.reserve_world_entry_frame(),
            Some(crate::WORLD_ENTRY_FRAME_FUEL)
        );
        assert_eq!(
            budget.remaining(),
            mod_api::TOTAL_MOD_FRAME_FUEL.saturating_sub(crate::WORLD_ENTRY_FRAME_FUEL)
        );
    }

    #[test]
    fn world_entry_reservations_refund_actual_use_and_carry_excess_across_frames() {
        for spent in [
            crate::FRAME_FUEL / 2,
            crate::FRAME_FUEL * 3,
            crate::WORLD_ENTRY_FRAME_FUEL,
        ] {
            let pool = mod_api::TOTAL_MOD_FRAME_FUEL;
            let budget = CallbackBudget::new(pool);
            assert_eq!(
                budget.reserve(crate::WORLD_ENTRY_FRAME_FUEL),
                Some(crate::WORLD_ENTRY_FRAME_FUEL)
            );
            budget.refund(crate::WORLD_ENTRY_FRAME_FUEL - spent);
            assert_eq!(budget.remaining(), pool.saturating_sub(spent));
            assert_eq!(budget.reserve(CALLBACK_FUEL), Some(CALLBACK_FUEL));
            assert_eq!(budget.reserve(CALLBACK_FUEL), None);
            assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
            budget.reset(pool);
            assert_eq!(budget.remaining(), pool - spent.saturating_sub(pool));
            budget.reset(pool);
            assert_eq!(budget.remaining(), pool);
        }
    }

    #[test]
    fn inexpensive_callbacks_reuse_refunded_fuel_without_extra_reservations() {
        let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
        let spent = crate::FRAME_FUEL / 8;
        for _ in 0..3 {
            assert_eq!(budget.reserve(crate::FRAME_FUEL), Some(crate::FRAME_FUEL));
            budget.refund(crate::FRAME_FUEL - spent);
        }
        assert_eq!(
            budget.remaining(),
            mod_api::TOTAL_MOD_FRAME_FUEL - spent * 3
        );
    }
}
