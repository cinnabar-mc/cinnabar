//! Owned continuations keep the guest and its transaction inaccessible between frame polls.

use super::*;
use crate::frame_time::Slice;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

type FrameFuture = Pin<Box<dyn Future<Output = (Store<State>, Result<()>, u64)> + Send>>;

/// Reads exact fuel after the borrowed call is dropped, including cancellation while pending.
struct InvocationStore {
    store: Option<Store<State>>,
    reservation: crate::callback_budget::FrameReservation,
    allowance: u64,
}

impl Drop for InvocationStore {
    /// Settles only the consumed fuel when an unfinished invocation loses its owner.
    fn drop(&mut self) {
        if let Some(store) = &self.store {
            let used = self.allowance - store.get_fuel().unwrap_or(0).min(self.allowance);
            self.reservation.settle(used);
        }
    }
}

/// Owns all partially executed guest state, including the post-return continuation.
pub(super) struct Continuation {
    // Only exclusive access polls the future; the mutex preserves resource thread safety.
    future: std::sync::Mutex<FrameFuture>,
    reservation: crate::callback_budget::FrameReservation,
    origin_frame: u64,
    last_polled: Option<u64>,
    ui_generation: u64,
    refused_before: u64,
    grants: ModGrants,
    session: Arc<SessionData>,
    world: Option<(u64, i32)>,
    pub(super) observed_world: Option<u64>,
}

impl Instance {
    /// Identifies this guest independently of its current load-order position.
    pub(crate) fn frame_owner(&self) -> crate::FrameOwner {
        self.frame_owner
    }

    /// Discards the departed guest's scheduling account before admitting fresh memory.
    pub(super) fn renew_frame_owner(&mut self) {
        if let Some(budget) = &self.callback_budget {
            budget.forget_frame(self.frame_owner);
        }
        self.frame_owner = crate::FrameOwner::new();
    }

    /// Starts shared time accounting without borrowing the guest during its call.
    pub(super) fn callback_timing(
        &self,
    ) -> Option<(
        crate::frame_time::FrameTime,
        crate::FrameOwner,
        std::time::Duration,
    )> {
        self.callback_budget
            .as_ref()
            .map(|budget| (budget.time.clone(), self.frame_owner, budget.time.now()))
    }

    /// Charges mandatory work against the same soft budget used for deferrable frames.
    pub(super) fn charge_callback_time(
        &self,
        timing: Option<(
            crate::frame_time::FrameTime,
            crate::FrameOwner,
            std::time::Duration,
        )>,
    ) {
        if let Some((time, owner, started)) = timing {
            time.charge(owner, time.now().saturating_sub(started));
        }
    }

    /// Keeps stores with immediate decision exports available to native event delivery.
    pub(super) fn can_suspend(&self) -> bool {
        !self.exports.requires_synchronous_frames()
            && self
                .callback_budget
                .as_ref()
                .is_some_and(|budget| budget.time.enabled())
    }

    /// Cancels partially executed memory without counting a scheduling or authority fault.
    pub(super) fn cancel_continuation(&mut self) {
        if self.continuation.take().is_some() && self.status == InstanceStatus::Running {
            self.status = InstanceStatus::Cancelled;
        }
    }

    /// Services at most one slice in this host frame, ignoring guest self-wakes after expiry.
    pub(super) fn run_frame_slice(&mut self) -> Result<CallbackOutcome> {
        if self.status != InstanceStatus::Running {
            return Ok(CallbackOutcome::Deferred);
        }
        let budget = self
            .callback_budget
            .clone()
            .expect("scheduled frame budget");
        let owner = self.frame_owner;
        let Some(slice) = budget.time.slice(owner) else {
            self.last_fuel = 0;
            return Ok(if self.continuation.is_some() {
                CallbackOutcome::Suspended
            } else {
                CallbackOutcome::Deferred
            });
        };
        if self
            .continuation
            .as_ref()
            .is_some_and(|call| call.last_polled == Some(slice.frame))
        {
            return Ok(CallbackOutcome::Suspended);
        }
        if self.continuation.as_ref().is_some_and(|call| {
            let state = self.store.data();
            call.grants != state.grants
                || !Arc::ptr_eq(&call.session, &state.session)
                || call
                    .world
                    .zip(state.snapshot.as_ref())
                    .is_some_and(|(old, frame)| old != (frame.session, frame.dimension))
        }) {
            self.cancel_continuation();
            return Ok(CallbackOutcome::Deferred);
        }
        let entry = self.continuation.is_none() && self.world_entry.available(self.store.data());
        let admission = if let Some(call) = &self.continuation {
            if !call.reservation.resume() {
                self.last_fuel = 0;
                return Ok(CallbackOutcome::Suspended);
            }
            None
        } else {
            let Some(admission) = budget.reserve_resumable_frame(entry) else {
                self.last_fuel = 0;
                return Ok(CallbackOutcome::Deferred);
            };
            Some(admission)
        };
        let started = slice.time.now();
        if let Some((allowance, reservation)) = admission {
            if entry && allowance == crate::WORLD_ENTRY_FRAME_FUEL {
                self.world_entry.consume();
            }
            // Only immutable snapshots and private owner transactions enter the guest store.
            // The facade contains no guest instance and is never entered while suspended.
            let committed = Store::new(self.pre.engine(), self.store.data().clone());
            let store = std::mem::replace(&mut self.store, committed);
            let mut invocation = InvocationStore {
                store: Some(store),
                reservation: reservation.clone(),
                allowance,
            };
            let store = invocation.store.as_mut().expect("invocation store");
            store.data_mut().begin_callback();
            crate::execution::arm_resumable(store, allowance)?;
            let exports = self.exports.clone();
            let state = store.data();
            let refused_before = state.exclusive.refused;
            let grants = state.grants.clone();
            let session = state.session.clone();
            let world = state
                .snapshot
                .as_ref()
                .map(|frame| (frame.session, frame.dimension));
            let state_world = state.player_state.world.as_ref().map(|world| world.session);
            self.continuation = Some(Continuation {
                future: std::sync::Mutex::new(Box::pin(async move {
                    let store = invocation.store.as_mut().expect("invocation store");
                    let result = exports.frame_async(&mut *store).await;
                    let used = allowance - store.get_fuel().unwrap_or(0).min(allowance);
                    invocation.reservation.settle(used);
                    let store = invocation.store.take().expect("completed invocation store");
                    (store, result, used)
                })),
                reservation,
                origin_frame: slice.frame,
                last_polled: None,
                ui_generation: self.ui_generation,
                refused_before,
                grants,
                session,
                world,
                observed_world: state_world,
            });
            // Input already frozen into this invocation must never be replayed by another.
            self.store.data_mut().controls.finish_text_frame(false);
            self.store.data_mut().touch.finish_frame(false);
        }
        let call = self
            .continuation
            .as_mut()
            .expect("started frame invocation");
        call.last_polled = Some(slice.frame);
        let polled = poll_slice(
            call.future.get_mut().expect("exclusive continuation"),
            &slice,
            started,
        );
        let outcome = match polled {
            Poll::Pending => {
                self.last_fuel = 0;
                CallbackOutcome::Suspended
            }
            Poll::Ready((store, result, used)) => {
                let call = self
                    .continuation
                    .take()
                    .expect("completed frame invocation");
                let current_frame = call.origin_frame == slice.frame;
                let same_ui = call.ui_generation == self.ui_generation;
                let native = std::mem::replace(&mut self.store, store).into_data();
                let transaction = std::mem::replace(self.store.data_mut(), native);
                self.last_fuel = used;
                if let Err(error) = result {
                    slice
                        .time
                        .charge(owner, slice.time.now().saturating_sub(started));
                    slice.time.completed(owner);
                    return Err(self.callback_failure(FRAME_FUEL, error));
                }
                self.store.data_mut().accept_frame(
                    transaction,
                    current_frame,
                    same_ui,
                    call.refused_before,
                );
                commit_frame_outputs(&mut self.store, true);
                self.consecutive_frame_overruns = 0;
                CallbackOutcome::Executed
            }
        };
        slice
            .time
            .charge(owner, slice.time.now().saturating_sub(started));
        if outcome == CallbackOutcome::Executed {
            slice.time.completed(owner);
        }
        Ok(outcome)
    }
}

impl Drop for Instance {
    /// Retires scheduling state alongside the instance's continuation and transaction.
    fn drop(&mut self) {
        if let Some(budget) = &self.callback_budget {
            budget.forget_frame(self.frame_owner);
        }
    }
}

/// Polls through cooperative fuel yields only until this owner exhausts its soft time share.
fn poll_slice<T>(
    future: &mut Pin<Box<dyn Future<Output = T> + Send>>,
    slice: &Slice,
    started: std::time::Duration,
) -> Poll<T> {
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Poll::Ready(value);
        }
        if slice.time.now().saturating_sub(started) >= slice.allowance {
            return Poll::Pending;
        }
    }
}

#[cfg(test)]
#[path = "suspension_tests.rs"]
mod tests;
