//! A continuation owns unsettled fuel; only measured use can borrow from later frames.

use super::*;

struct Reservation {
    paid: u64,
    frame: u64,
    current_paid: u64,
    settled: bool,
}

/// Tracks conservative reservations until the owning store exposes its exact consumption.
#[derive(Clone)]
pub(crate) struct FrameReservation {
    budget: CallbackBudget,
    state: Arc<Mutex<Reservation>>,
}

impl CallbackBudget {
    /// Admits a full guest limit, postponing world-entry borrowing until use is known.
    pub(crate) fn reserve_resumable_frame(
        &self,
        world_entry: bool,
    ) -> Option<(u64, FrameReservation)> {
        let mut pool = self.allowance.lock().expect("mod callback budget");
        if pool.frame < crate::FRAME_FUEL {
            return None;
        }
        let limit = if world_entry && pool.world_entries != 0 {
            pool.world_entries -= 1;
            crate::WORLD_ENTRY_FRAME_FUEL
        } else {
            crate::FRAME_FUEL
        };
        let paid = limit.min(pool.frame);
        pool.frame -= paid;
        Some((
            limit,
            FrameReservation {
                budget: self.clone(),
                state: Arc::new(Mutex::new(Reservation {
                    paid,
                    frame: pool.generation,
                    current_paid: paid,
                    settled: false,
                })),
            },
        ))
    }
}

impl FrameReservation {
    /// Reserves one more service frame without replenishing the guest's invocation fuel.
    pub(crate) fn resume(&self) -> bool {
        let mut state = self.state.lock().expect("mod frame reservation");
        let mut pool = self.budget.allowance.lock().expect("mod callback budget");
        if state.settled || pool.frame < crate::FRAME_FUEL {
            return false;
        }
        pool.frame -= crate::FRAME_FUEL;
        state.paid = state.paid.saturating_add(crate::FRAME_FUEL);
        if state.frame != pool.generation {
            state.frame = pool.generation;
            state.current_paid = 0;
        }
        state.current_paid = state.current_paid.saturating_add(crate::FRAME_FUEL);
        true
    }

    /// Settles exact consumption on completion or cancellation; expired shares never accrue.
    pub(crate) fn settle(&self, used: u64) {
        let mut state = self.state.lock().expect("mod frame reservation");
        if state.settled {
            return;
        }
        state.settled = true;
        let mut pool = self.budget.allowance.lock().expect("mod callback budget");
        if used > state.paid {
            let extra = used - state.paid;
            pool.debt += extra.saturating_sub(pool.frame);
            pool.frame = pool.frame.saturating_sub(extra);
        } else if state.frame == pool.generation {
            let refund = (state.paid - used).min(state.current_paid);
            pool.frame += refund.saturating_sub(pool.debt);
            pool.debt = pool.debt.saturating_sub(refund);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_world_entry_leaves_only_its_actual_fuel_charge() {
        let pool = mod_api::TOTAL_MOD_FRAME_FUEL;
        for advance in [false, true] {
            let budget = CallbackBudget::new(pool);
            let (limit, reservation) = budget.reserve_resumable_frame(true).unwrap();
            assert_eq!(limit, crate::WORLD_ENTRY_FRAME_FUEL);
            if advance {
                budget.reset(pool);
            }
            let used = crate::FRAME_FUEL / 8;
            reservation.settle(used);
            assert_eq!(budget.remaining(), if advance { pool } else { pool - used });
            budget.reset(pool);
            assert_eq!(budget.remaining(), pool);
        }
    }

    #[test]
    fn resumption_refunds_the_current_reservation_without_carrying_expired_shares() {
        let pool = mod_api::TOTAL_MOD_FRAME_FUEL;
        let budget = CallbackBudget::new(pool);
        let (_, reservation) = budget.reserve_resumable_frame(false).unwrap();
        budget.reset(pool);
        assert!(reservation.resume());
        reservation.settle(crate::FRAME_FUEL / 8);
        assert_eq!(budget.remaining(), pool);
        reservation.settle(0);
        assert_eq!(budget.remaining(), pool);
    }

    #[test]
    fn only_consumed_world_entry_excess_reduces_later_capacity() {
        let pool = mod_api::TOTAL_MOD_FRAME_FUEL;
        let budget = CallbackBudget::new(pool);
        let (_, reservation) = budget.reserve_resumable_frame(true).unwrap();
        let used = pool + crate::FRAME_FUEL / 2;
        reservation.settle(used);
        assert_eq!(budget.remaining(), 0);
        budget.reset(pool);
        assert_eq!(budget.remaining(), pool - (used - pool));
    }
}
