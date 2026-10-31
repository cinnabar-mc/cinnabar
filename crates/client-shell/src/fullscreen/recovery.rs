//! Orders durable fullscreen attribution around native mode changes without blocking frames.

use super::*;
use bevy::{
    app::OnAppExitSystems,
    prelude::{App, AppExit, IntoScheduleConfigs, Last, Resource, Startup, Update},
};
use launcher_host::renderer_startup::fullscreen::FullscreenGuard;
use render_scene::startup_presentation::StartupPresentation;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

type Save = std::thread::JoinHandle<(bool, Result<(), String>)>;
type Open = std::thread::JoinHandle<Result<FullscreenGuard, String>>;

/// What an armed journal still waits for before it may be cleared.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Completion {
    /// The mode was assigned, but the platform has not yet settled into it.
    Native(bool),
    /// The platform settled; a presentation extracted afterwards must complete.
    Frame(u64),
    /// Only the journal removal remains.
    Done,
}

/// Keeps native entry behind its journal write and completion behind a settled presentation.
#[derive(Default, Resource)]
pub struct FullscreenTransition {
    root: Option<PathBuf>, // Unset when recovery does not cover the primary window.
    guard: Option<Arc<Mutex<FullscreenGuard>>>,
    opening: Option<Open>,
    saving: Option<Save>,
    armed: bool,
    requested: Option<bool>,
    completion: Option<Completion>,
    blocked: bool, // Storage failed; the next request retries.
}

/// Installs recovery before settings apply and cancellation after orderly exit requests.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<FullscreenTransition>()
        .init_resource::<NativeFullscreen>()
        .add_systems(Startup, initialize)
        .add_systems(
            Update,
            super::native::sample.before(apply_runtime_fullscreen_setting),
        )
        .add_systems(Last, cancel_on_exit.in_set(OnAppExitSystems));
}

/// Reads recovery before the first visible window can apply a retained fullscreen preference.
fn initialize(
    mut transition: ResMut<FullscreenTransition>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
) {
    // Android's native window ignores mode requests, so it has no transition to attribute.
    if launcher::FULLSCREEN_ONLY
        || cfg!(target_os = "android")
        || !windows.iter().any(|window| window.visible)
    {
        return;
    }
    let root = menu.layout().user_config_root.clone();
    match FullscreenGuard::open(&root) {
        Ok(guard) => {
            if guard.recovered {
                menu.take_fullscreen_change();
                menu.sync_fullscreen(false);
                settings.set_fullscreen(false);
            }
            transition.guard = Some(Arc::new(Mutex::new(guard)));
        }
        // Entry opens again once requested, waiting out another client's transition.
        Err(error) => bevy::log::warn!("Fullscreen recovery is not open yet: {error:#}"),
    }
    transition.root = Some(root);
}

impl FullscreenTransition {
    /// Retains the newest input while the preceding durable write is still running.
    pub(super) fn request(&mut self, fullscreen: bool) {
        // New intent retries storage or a lease that failed for an earlier request.
        self.blocked = false;
        self.requested = Some(fullscreen);
    }

    /// Reports pending intent so later frames and hotkeys do not restore the old window mode.
    pub(super) fn requested(&self) -> Option<bool> {
        self.requested
    }

    /// Publishes a completed write, retaining attribution if storage reported an error.
    fn finish_save(&mut self) {
        let Some(worker) = self.saving.take() else {
            return;
        };
        match worker.join() {
            Ok((armed, result)) => match result {
                Ok(()) => {
                    self.armed = armed;
                    if !armed {
                        self.completion = None;
                    }
                }
                Err(error) => {
                    self.blocked = true;
                    bevy::log::warn!("Fullscreen recovery could not save: {error}");
                }
            },
            Err(_) => self.blocked = true,
        }
    }

    /// Opens recovery off the frame thread; it waits while another client owns a transition.
    fn open(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        match std::thread::Builder::new()
            .name("fullscreen-recovery-open".into())
            .spawn(move || {
                FullscreenGuard::open_waiting(&root).map_err(|error| format!("{error:#}"))
            }) {
            Ok(worker) => self.opening = Some(worker),
            Err(error) => {
                self.blocked = true;
                bevy::log::warn!("Fullscreen recovery could not start opening: {error}");
            }
        }
    }

    /// Adopts an opened journal; false while the open still waits for another client.
    fn finish_open(&mut self) -> bool {
        if self
            .opening
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return false;
        }
        let Some(worker) = self.opening.take() else {
            return true;
        };
        match worker.join() {
            Ok(Ok(guard)) => {
                // Another client's interrupted entry reset the shared preference; stay windowed.
                if guard.recovered {
                    self.requested = None;
                }
                self.guard = Some(Arc::new(Mutex::new(guard)));
            }
            Ok(Err(error)) => {
                self.blocked = true;
                bevy::log::warn!("Fullscreen recovery could not open: {error}");
            }
            Err(_) => self.blocked = true,
        }
        true
    }

    /// Starts one ordered storage operation; callers poll its handle without waiting.
    fn save(&mut self, armed: bool) {
        self.save_with(armed, |guard, armed| {
            std::thread::Builder::new()
                .name("fullscreen-recovery".into())
                .spawn(move || {
                    let mut guard = guard.lock().unwrap_or_else(|error| error.into_inner());
                    let result = if armed { guard.begin() } else { guard.clear() };
                    (armed, result.map_err(|error| format!("{error:#}")))
                })
        });
    }

    /// Retains journal ownership even when the injected worker launcher returns an error.
    fn save_with(
        &mut self,
        armed: bool,
        spawn: impl FnOnce(Arc<Mutex<FullscreenGuard>>, bool) -> std::io::Result<Save>,
    ) {
        let Some(guard) = self.guard.as_ref().map(Arc::clone) else {
            return;
        };
        match spawn(Arc::clone(&guard), armed) {
            Ok(worker) => self.saving = Some(worker),
            Err(error) => {
                self.blocked = true;
                bevy::log::warn!("Fullscreen recovery worker could not start: {error}");
                // A successful transition must not retain crash attribution just because
                // the cleanup worker could not start. This fallback only unlinks the marker.
                if !armed {
                    match guard
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clear()
                    {
                        Ok(()) => {
                            self.armed = false;
                            self.completion = None;
                            self.blocked = false;
                        }
                        Err(error) => {
                            bevy::log::warn!("Fullscreen recovery could not clear: {error:#}")
                        }
                    }
                }
            }
        }
    }

    /// Applies only journaled entry, and clears it only after the platform settled and a
    /// presentation extracted afterwards completed. `native` is the platform's settled state.
    pub(super) fn update(
        &mut self,
        window: &mut Window,
        feedback: Option<&StartupPresentation>,
        native: Option<bool>,
    ) {
        if self.root.is_none() {
            if let Some(fullscreen) = self.requested.take()
                && fullscreen != is_fullscreen(window)
            {
                window.mode = window_mode(fullscreen);
            }
            return;
        }
        if !self.finish_open()
            || self
                .saving
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        self.finish_save();
        if self.guard.is_none() && !self.blocked && self.requested == Some(true) {
            self.open();
            return;
        }
        if self.blocked || self.guard.is_none() {
            if self.requested == Some(false) {
                window.mode = window_mode(false);
                self.requested = None;
            }
            return;
        }
        if let Some(fullscreen) = self.requested {
            if fullscreen && !self.armed && !is_fullscreen(window) {
                self.save(true);
                return;
            }
            self.requested = None;
            if fullscreen != is_fullscreen(window) {
                window.mode = window_mode(fullscreen);
                if self.armed {
                    // This frame's native sample predates the request, so it cannot confirm it.
                    self.completion = Some(Completion::Native(fullscreen));
                    return;
                }
            }
        }
        if !self.armed {
            return;
        }
        match self.completion {
            Some(Completion::Native(fullscreen)) if native == Some(fullscreen) => {
                if let Some(feedback) = feedback {
                    self.completion = Some(Completion::Frame(feedback.begin_mode_change()));
                }
            }
            Some(Completion::Frame(epoch))
                if feedback.is_some_and(|feedback| feedback.mode_change_completed(epoch)) =>
            {
                self.completion = Some(Completion::Done);
            }
            None if !is_fullscreen(window) => self.completion = Some(Completion::Done),
            _ => {}
        }
        if self.completion == Some(Completion::Done) {
            self.save(false);
        }
    }
}

/// Joins only on orderly shutdown, cancelling even an entry write that has not finished yet.
fn cancel_on_exit(mut exits: MessageReader<AppExit>, mut transition: ResMut<FullscreenTransition>) {
    if exits.read().next().is_none() {
        return;
    }
    transition.finish_save();
    if let Some(guard) = transition.guard.as_ref()
        && let Err(error) = guard
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear()
    {
        bevy::log::warn!("Fullscreen recovery could not cancel: {error:#}");
    }
}

impl Drop for FullscreenTransition {
    /// Releases the worker's lease without clearing an unhandled interruption's attribution.
    fn drop(&mut self) {
        self.finish_save();
    }
}

#[cfg(test)]
mod tests;
