//! Saves and stops the open world's server once it has kept running for a while with the app off
//! screen.
//!
//! A local server cannot pause, so while the app is hidden it keeps simulating the player's world
//! and holding its memory. Android may end a hidden app at any moment, and its helper processes go
//! with it unsaved. A short absence, such as answering a message, keeps the world; a longer one
//! saves it and frees the server's memory while the app can still do so. Time the app spends
//! frozen by the system is not counted: a frozen server neither simulates nor allocates, and
//! counting that time would close the world just as the player returns to it.

use std::{
    thread,
    time::{Duration, Instant},
};

use bridge::WorldState;
use crossbeam_channel::{RecvTimeoutError, Sender};

use super::Manager;
use crate::local_worlds::lock;

/// How long the open world's server may run while the app is hidden before it is saved and
/// stopped.
pub(super) const RUN_LIMIT: Duration = Duration::from_secs(60);
/// How often a hidden app's watch counts the time its server ran.
pub(super) const POLL: Duration = Duration::from_secs(1);

/// Whether the app is off screen, and what happened to the open world meanwhile.
#[derive(Default)]
pub(super) struct Hidden {
    /// The platform has taken the app off screen.
    app_hidden: bool,
    /// The running watch's number and the sender that ends it when dropped; `None` while the
    /// app is on screen.
    watch: Option<(u64, Sender<()>)>,
    /// Numbers watches, so a watch the app's return has ended never closes a later one's world.
    watches: u64,
    /// The watch closed a starting or running world.
    closed: bool,
}

/// The time a hidden app's process actually ran, from the observations of a watch that wakes
/// every `step`. A frozen process's threads do not run, so a longer gap between two observations
/// counts as one step, never as the time the process was frozen.
#[derive(Clone, Copy, Debug)]
pub(super) struct HiddenRun {
    ran: Duration,
    last: Instant,
    step: Duration,
}

impl HiddenRun {
    /// Starts counting at `now`.
    pub(super) fn new(now: Instant, step: Duration) -> Self {
        Self {
            ran: Duration::ZERO,
            last: now,
            step,
        }
    }

    /// Counts the time since the previous observation, at most one step, and returns the total.
    pub(super) fn observe(&mut self, now: Instant) -> Duration {
        self.ran += now.saturating_duration_since(self.last).min(self.step);
        self.last = now;
        self.ran
    }
}

impl Manager {
    /// Starts counting the time the open world's server runs while the platform keeps the app off
    /// screen; once it reaches the run limit, the world is saved and stopped, or its start
    /// cancelled. A world opened while hidden is watched from its open.
    pub fn app_hidden(&self) {
        lock(&self.inner.hidden).app_hidden = true;
        self.watch_if_hidden();
    }

    /// Starts the hidden-app watch when the app is hidden, no watch runs and a world is starting
    /// or running.
    pub(super) fn watch_if_hidden(&self) {
        let mut hidden = lock(&self.inner.hidden);
        if !hidden.app_hidden || hidden.watch.is_some() {
            return;
        }
        let state = lock(&self.inner.life).state;
        if !matches!(state, WorldState::Starting | WorldState::Running) {
            return;
        }
        let (watch, shown) = crossbeam_channel::bounded::<()>(0);
        hidden.watches += 1;
        let number = hidden.watches;
        let inner = std::sync::Arc::downgrade(&self.inner);
        let (limit, poll) = (self.inner.hidden_run_limit, self.inner.hidden_poll);
        let spawned = thread::Builder::new()
            .name("local-world-hidden".to_owned())
            .spawn(move || {
                let mut run = HiddenRun::new(Instant::now(), poll);
                loop {
                    // A message or a dropped sender both mean the app returned or the manager is gone.
                    if !matches!(shown.recv_timeout(poll), Err(RecvTimeoutError::Timeout)) {
                        return;
                    }
                    if run.observe(Instant::now()) >= limit {
                        break;
                    }
                }
                if let Some(inner) = inner.upgrade() {
                    Self { inner }.close_hidden(number);
                }
            });
        match spawned {
            Ok(_) => hidden.watch = Some((number, watch)),
            Err(error) => tracing::warn!("watch the local world while hidden: {error}"),
        }
    }

    /// Ends the hidden-app watch as the platform brings the app back; true when it closed the
    /// world meanwhile, which is reported once.
    pub fn app_shown(&self) -> bool {
        let mut hidden = lock(&self.inner.hidden);
        hidden.app_hidden = false;
        hidden.watch = None;
        std::mem::take(&mut hidden.closed)
    }

    /// Whether the watch closed the world since the app was hidden, without ending the report
    /// [`Self::app_shown`] makes.
    #[must_use]
    pub fn closed_while_hidden(&self) -> bool {
        lock(&self.inner.hidden).closed
    }

    /// Closes the open world for watch `number`, unless the app has returned since it started.
    fn close_hidden(&self, number: u64) {
        let mut hidden = lock(&self.inner.hidden);
        if hidden.watch.as_ref().map(|(watch, _)| *watch) != Some(number) {
            return;
        }
        // This watch has ended, so a later open while still hidden starts its own.
        hidden.watch = None;
        let life = lock(&self.inner.life);
        if !matches!(life.state, WorldState::Starting | WorldState::Running) {
            return;
        }
        tracing::info!(
            state = ?life.state,
            world = life.world.as_ref().map(|world| world.id.as_str()),
            "local world closed: its server kept running while the app was off screen"
        );
        hidden.closed = true;
        self.close_locked(life);
    }
}
