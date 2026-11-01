//! Explicit container pointer writes after the window backend releases capture.

use bevy::{
    app::MainScheduleOrder,
    ecs::{schedule::ScheduleLabel, system::NonSendMarker},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
    winit::WINIT_WINDOWS,
};

use super::*;

/// One committed request, retained until capture releases or its container loses authority.
#[derive(Resource, Default)]
pub(super) struct Pending(Option<Request>);

struct Request {
    owner: usize,
    instance: u64,
    screen: &'static str,
    point: Vec2,
}

impl Pending {
    /// Waits for capture release, but immediately cancels requests whose authority departed.
    fn take(&mut self, authorized: bool, desktop: bool) -> Option<Request> {
        if !authorized {
            self.0 = None;
        }
        if desktop { self.0.take() } else { None }
    }

    /// Keeps the admitted owner's explicit write separate from cached window state.
    pub(super) fn stage(&mut self, owner: usize, instance: u64, screen: &'static str, point: Vec2) {
        self.0 = Some(Request {
            owner,
            instance,
            screen,
            point,
        });
    }
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct ApplyNativePointer;

/// Runs after both cached window updates and native cursor capture changes in Last.
fn install_schedule(app: &mut App) {
    app.init_resource::<Backend>()
        .init_resource::<Pending>()
        .init_schedule(ApplyNativePointer);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(Last, ApplyNativePointer);
}

/// Installs the main-thread native commit after the backend's ordinary window updates.
pub(super) fn configure(app: &mut App) {
    install_schedule(app);
    app.add_systems(ApplyNativePointer, apply);
}

/// Current authority is checked again after all application and backend systems have run.
#[derive(SystemParam)]
struct Authority<'w, 's> {
    windows: Query<'w, 's, (Entity, &'static Window, &'static CursorOptions), With<PrimaryWindow>>,
    runtime: Option<Res<'w, ModRuntime>>,
    player: Res<'w, client_state::PlayerRuntime>,
    ui: Res<'w, UiRuntimeResource>,
    presentation: Res<'w, UiPresentationResource>,
    menu: Option<Res<'w, MenuRuntime>>,
    focus: Option<Res<'w, client_state::CursorFocus>>,
    driven: Option<Res<'w, client_state::DrivenInput>>,
}

/// Synthetic input and a captured, hidden or unfocused desktop cursor cannot be warped.
fn desktop_available(window: &Window, cursor: &CursorOptions, driven: bool) -> bool {
    !driven && window.focused && cursor.visible && cursor.grab_mode == CursorGrabMode::None
}

/// Submits every accepted write explicitly, even when its coordinates match the backend cache.
fn apply(
    mut pending: ResMut<Pending>,
    mut backend: ResMut<Backend>,
    authority: Authority,
    _main_thread: NonSendMarker,
) {
    let Some(runtime) = authority.runtime.as_deref() else {
        pending.0 = None;
        return;
    };
    let Some(request) = pending.0.as_ref() else {
        return;
    };
    let Ok((entity, window, cursor)) = authority.windows.single() else {
        pending.0 = None;
        return;
    };
    let invalid = authority.driven.is_some()
        || (!cursor.visible && cursor.grab_mode == CursorGrabMode::None)
        || runtime.suspended
        || request.owner >= runtime.hosts.host_count()
        || runtime.hosts.host(request.owner).touch_owner() != request.instance
        || !runtime.hosts.host(request.owner).is_active()
        || !runtime.hosts.host(request.owner).grants().controls
        || runtime.hosts.host(runtime.hosts.panel_owner()).panel_open()
        || runtime
            .hosts
            .screen_owner()
            .is_some_and(|owner| runtime.hosts.host(owner).screens().view.is_some())
        || drive::observed_container_screen(&authority.player, &authority.ui)
            != Some(request.screen)
        || !drive::container_pointer_available(
            &authority.player,
            &authority.ui,
            &authority.presentation,
            authority
                .menu
                .as_deref()
                .is_some_and(MenuRuntime::is_visible),
            drive::pointer_focus_available(window.focused, authority.focus.as_deref(), false),
        );
    let Some(request) = pending.take(!invalid, desktop_available(window, cursor, false)) else {
        return;
    };
    let point = request.point.as_dvec2() * f64::from(window.resolution.scale_factor());
    if let Err(error) = backend.0.position(entity, point) {
        debug!(%error, "Could not apply container pointer position");
    }
}

/// Native delivery can be recorded without replacing the production authority checks.
trait NativePointer: Send + Sync + 'static {
    /// Delivers one explicit move in physical pixels on the event-loop thread.
    fn position(&mut self, window: Entity, point: bevy::math::DVec2) -> Result<(), String>;
}

#[derive(Resource)]
struct Backend(Box<dyn NativePointer>);
impl Default for Backend {
    /// Uses the real window backend outside tests.
    fn default() -> Self {
        Self(Box::new(Desktop))
    }
}
struct Desktop;
impl NativePointer for Desktop {
    /// Sends every explicit request, including coordinates already in the window cache.
    fn position(&mut self, window: Entity, point: bevy::math::DVec2) -> Result<(), String> {
        WINIT_WINDOWS.with_borrow(|windows| {
            let Some(native) = windows.get_window(window) else {
                return Ok(());
            };
            native
                .set_cursor_position(winit::dpi::PhysicalPosition::new(point.x, point.y))
                .map_err(|error| error.to_string())
        })
    }
}

#[cfg(test)]
mod tests;
