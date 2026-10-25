//! Reference interpreter for server writes and ordered bow-button lifetimes.

use super::*;
use std::collections::VecDeque;

/// The interpreter records accepted use ownership and the last unacknowledged server write.
#[derive(Default)]
struct Model {
    owned: Option<bool>,
    server: Option<bool>,
    stop_ticked: bool,
    release_owed: bool,
    release_seen: bool,
    projected_release: bool,
    repeat_armed: bool,
}

impl Model {
    /// A real held-button observation can take ownership of an existing use.
    fn observe(&mut self, held: bool) {
        if let Some(owned) = self.owned.as_mut() {
            self.release_seen |= *owned && !held;
            *owned |= held;
        }
    }

    /// Input runs before tick reconciliation; queue refusal retains only an accepted release.
    fn input(&mut self, held: bool, pressed: bool, full: bool) {
        self.observe(held);
        let release = self.release_owed || self.release_seen;
        let armed = pressed || self.repeat_armed;
        let start = (pressed || held && armed) && (self.owned.is_none() || release);
        if full && (release || start) {
            self.release_owed |= release;
            return;
        }
        self.repeat_armed = held && armed;
        if release || start {
            self.owned = start.then_some(true);
            self.server = None;
            self.stop_ticked = false;
            self.release_owed = false;
            self.release_seen = false;
            self.projected_release = false;
        }
    }

    /// Only the latest server write matters at a tick; earlier unresolved releases run first.
    fn tick(&mut self, held: bool, edges: &VecDeque<bool>) {
        self.observe(held);
        let mut owned = self.owned == Some(true);
        self.projected_release = !self.release_seen
            && edges.iter().copied().any(|held| {
                let released = owned && !held;
                owned |= held && self.owned.is_some();
                released
            });
        match self.server {
            Some(true) => {
                self.owned.get_or_insert(held);
                self.server = None;
            }
            Some(false) => {
                if self.projected_release || self.release_seen || self.release_owed {
                    self.stop_ticked = true;
                } else {
                    self.owned = None;
                    self.server = None;
                    self.stop_ticked = false;
                }
            }
            None => {}
        }
    }

    /// Expired gestures cannot replace a ticked stop; admitted releases retain their priority.
    fn finish(&mut self) {
        if self.projected_release && self.stop_ticked && !self.release_owed && !self.release_seen {
            self.owned = None;
            self.server = None;
            self.stop_ticked = false;
        }
        self.projected_release = false;
    }
}

/// Runs both interpreters with the same queue, logical clock and transport conditions.
#[derive(Default)]
struct Trial {
    runtime: ItemUseRuntime,
    swings: SwingTracker,
    model: Model,
    edges: VecDeque<bool>,
    held: bool,
    final_held: bool,
    full: bool,
    selection_barrier: bool,
    tick: u64,
    /// A server clear that no later write or local start or end has superseded.
    clear_owed: bool,
}

impl Trial {
    /// Gives every action a fresh tick outside the bow's press-rearm interval.
    fn frame(&mut self, held: bool) -> UseFrame {
        self.tick += 10;
        frame(self.tick, held)
    }

    /// Resolves one edge with disposable press state, as an ordered dispatcher does.
    fn input(&mut self, held: bool, pressed: bool) {
        let frame = self.frame(held);
        self.runtime.observe_use_button(held);
        let mut pending = PendingUseInput::new(pressed);
        self.runtime.swap_pending_input(&mut pending);
        let full = self.full;
        step_and_send(&mut self.runtime, &mut self.swings, &frame, 42, 6, |_| {
            if full {
                Err(BatchSendError::Full)
            } else {
                Ok(())
            }
        });
        self.runtime.swap_pending_input(&mut pending);
        self.model.input(held, pressed, full);
    }

    /// Drains one edge unless an older accepted release or selection blocks it.
    fn drain_one(&mut self) {
        if self.model.release_owed || self.model.release_seen || self.full && self.selection_barrier
        {
            return;
        }
        if let Some(held) = self.edges.pop_front() {
            self.held = held;
            self.input(held, held);
        }
    }

    /// Ends the final drain, expiring its blocked suffix before held maintenance.
    fn finish(&mut self) {
        self.edges.clear();
        self.held = self.final_held;
        self.runtime.finish_ordered_use_buttons();
        self.model.finish();
    }

    /// Checks accepted activity and both outstanding obligations after every event.
    fn check(&mut self, seed: u64, step: usize, event: u64) {
        let context = format!("seed={seed:#x}, step={step}, event={event}");
        // Independent of the model: a running use keeps an unsuperseded clear pending.
        if self.runtime.take_local_using_item().is_some() || !self.runtime.is_using() {
            self.clear_owed = false;
        }
        assert!(
            !self.clear_owed || self.runtime.has_server_using_item(),
            "a server clear was dropped while its use still runs: {context}"
        );
        assert_eq!(
            self.runtime.is_using(),
            self.model.owned.is_some(),
            "{context}"
        );
        assert_eq!(
            self.runtime.has_pending_release(),
            self.model.release_owed || self.model.release_seen,
            "{context}"
        );
        assert_eq!(
            self.runtime.has_server_using_item(),
            self.model.server.is_some(),
            "a server clear must be applied or retained: {context}"
        );
        assert_eq!(
            self.runtime.movement_modifier().is_some(),
            self.model.owned.is_some(),
            "{context}"
        );
    }
}

/// Random interleavings cover both queue barriers and transaction-level backpressure.
#[test]
fn seeded_item_use_authority_matches_the_reference_interpreter() {
    for seed in 1_u64..=32 {
        let mut random = seed;
        let mut trial = Trial::default();
        for step in 0..512 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005_u64)
                .wrapping_add(1);
            let event = random >> 32;
            match event % 9 {
                0 | 1 => {
                    let using = event % 9 == 0;
                    trial.runtime.adopt_server_using_item(Some(using));
                    trial.model.server = Some(using);
                    trial.model.stop_ticked = false;
                    trial.clear_owed = !using;
                }
                2 => {
                    trial.final_held = !trial.final_held;
                    trial.edges.push_back(trial.final_held);
                }
                3 => {
                    let frame = trial.frame(trial.held);
                    trial
                        .runtime
                        .observe_ordered_use_buttons(trial.held, trial.edges.iter().copied());
                    trial.runtime.resolve_server_using_item(&frame);
                    trial.model.tick(trial.held, &trial.edges);
                }
                4 => trial.drain_one(),
                5 => {
                    while !trial.edges.is_empty() {
                        let remaining = trial.edges.len();
                        trial.drain_one();
                        trial.check(seed, step, event % 9);
                        if trial.edges.len() == remaining {
                            break;
                        }
                    }
                    trial.finish();
                }
                6 if trial.edges.is_empty() => trial.input(trial.held, false),
                7 => trial.full = !trial.full,
                8 => trial.selection_barrier = !trial.selection_barrier,
                _ => {}
            }
            trial.check(seed, step, event % 9);
        }
        trial.full = false;
        trial.finish();
        trial.check(seed, 512, 9);
        trial.input(trial.held, false);
        trial.check(seed, 512, 9);
    }
}
