//! Orders durable fullscreen attribution around native mode changes without blocking frames.

use super::*;
use bevy::{
    app::OnAppExitSystems,
    prelude::{App, AppExit, IntoScheduleConfigs, Last, Res, Resource, Startup},
};
use launcher_host::renderer_startup::fullscreen::FullscreenGuard;
use render_scene::startup_presentation::StartupPresentation;
use std::sync::{Arc, Mutex};

type Save = std::thread::JoinHandle<(bool, Result<(), String>)>;

/// Keeps native entry behind its journal write and completion behind a new presentation.
#[derive(Default, Resource)]
pub struct FullscreenTransition {
    guard: Option<Arc<Mutex<FullscreenGuard>>>,
    saving: Option<Save>,
    armed: bool,
    requested: Option<bool>,
    presentation: Option<u64>,
    blocked: bool,
}

/// Installs recovery before settings apply and cancellation after orderly exit requests.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<FullscreenTransition>()
        .add_systems(Startup, initialize)
        .add_systems(Last, cancel_on_exit.in_set(OnAppExitSystems));
}

/// Reads recovery before the first visible window can apply a retained fullscreen preference.
fn initialize(
    mut transition: ResMut<FullscreenTransition>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
) {
    if launcher::FULLSCREEN_ONLY || !windows.iter().any(|window| window.visible) {
        return;
    }
    match FullscreenGuard::open(&menu.layout().user_config_root) {
        Ok(guard) => {
            if guard.recovered {
                menu.take_fullscreen_change();
                menu.sync_fullscreen(false);
                settings.set_fullscreen(false);
            }
            transition.guard = Some(Arc::new(Mutex::new(guard)));
        }
        Err(error) => {
            transition.blocked = true;
            bevy::log::warn!("Fullscreen recovery could not start: {error:#}");
        }
    }
}

impl FullscreenTransition {
    /// Retains the newest input while the preceding durable write is still running.
    pub(super) fn request(&mut self, fullscreen: bool) {
        if self.guard.is_some() && !self.armed {
            self.blocked = false;
        }
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
                Ok(()) => self.armed = armed,
                Err(error) => {
                    self.blocked = true;
                    bevy::log::warn!("Fullscreen recovery could not save: {error}");
                }
            },
            Err(_) => self.blocked = true,
        }
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

    /// Applies only journaled entry and rejects stale, already-submitted frame completions.
    pub(super) fn update(&mut self, window: &mut Window, feedback: Option<&StartupPresentation>) {
        if self
            .saving
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        self.finish_save();
        if self.blocked {
            if self.requested == Some(false) {
                window.mode = window_mode(false);
                self.requested = None;
            }
            return;
        }
        if let Some(fullscreen) = self.requested {
            if fullscreen == is_fullscreen(window) {
                self.requested = None;
            } else if fullscreen && self.guard.is_some() && !self.armed {
                self.save(true);
                return;
            } else if !fullscreen || self.guard.is_some() {
                window.mode = window_mode(fullscreen);
                self.requested = None;
                if self.armed {
                    self.presentation = feedback.map(StartupPresentation::begin_mode_change);
                }
            }
        }
        if self.armed
            && self.requested.is_none()
            && (self.presentation.is_some_and(|epoch| {
                feedback.is_some_and(|feedback| feedback.mode_change_completed(epoch))
            }) || (self.presentation.is_none() && !is_fullscreen(window)))
        {
            self.presentation = None;
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
