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
    app.init_resource::<Pending>()
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
    runtime: Res<'w, ModRuntime>,
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
fn apply(mut pending: ResMut<Pending>, authority: Authority, _main_thread: NonSendMarker) {
    let Some(request) = pending.0.as_ref() else {
        return;
    };
    let Ok((entity, window, cursor)) = authority.windows.single() else {
        pending.0 = None;
        return;
    };
    let runtime = &authority.runtime;
    let invalid = authority.driven.is_some()
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
    WINIT_WINDOWS.with_borrow(|windows| {
        if let Some(native) = windows.get_window(entity) {
            let point = request.point.as_dvec2() * f64::from(window.resolution.scale_factor());
            if let Err(error) =
                native.set_cursor_position(winit::dpi::PhysicalPosition::new(point.x, point.y))
            {
                warn!(%error, "Could not apply container pointer position");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An explicit write still reaches the native adapter when cached coordinates are equal.
    #[test]
    fn explicit_write_follows_backend_capture_release() {
        #[derive(Resource, Default)]
        struct Backend {
            captured: bool,
            point: Vec2,
            writes: Vec<Vec2>,
        }
        /// Queues the same coordinates the backend already remembers.
        fn request(mut pending: ResMut<Pending>, backend: Res<Backend>) {
            pending.stage(0, 1, "fixture.container", backend.point);
        }
        /// Models the backend applying its cursor options in Last.
        fn release(mut backend: ResMut<Backend>) {
            backend.captured = false;
        }
        /// Records an explicit OS write after capture release.
        fn write(mut pending: ResMut<Pending>, mut backend: ResMut<Backend>) {
            let request = pending.0.take().unwrap();
            assert!(
                !backend.captured,
                "native capture must be released before positioning"
            );
            backend.writes.push(request.point);
        }
        let mut app = App::new();
        install_schedule(&mut app);
        app.insert_resource(Backend {
            captured: true,
            point: Vec2::new(640., 400.),
            ..Default::default()
        })
        .add_systems(Update, request)
        .add_systems(Last, release)
        .add_systems(ApplyNativePointer, write);
        app.update();
        assert_eq!(
            app.world().resource::<Backend>().writes,
            vec![Vec2::new(640., 400.)]
        );
        assert!(app.world().resource::<Pending>().0.is_none());
    }

    /// A request waits for native capture release, and loses all authority on cancellation.
    #[test]
    fn pending_write_waits_without_outliving_its_authority() {
        let mut pending = Pending::default();
        pending.stage(0, 1, "fixture.container", Vec2::new(160., 90.));
        assert!(pending.take(true, false).is_none());
        assert_eq!(
            pending.take(true, true).unwrap().point,
            Vec2::new(160., 90.)
        );
        assert!(pending.take(true, true).is_none());
        pending.stage(0, 1, "fixture.container", Vec2::ZERO);
        assert!(pending.take(false, false).is_none());
        assert!(pending.take(true, true).is_none());
    }

    /// Desktop positioning cannot move a captured cursor or escape driven-input isolation.
    #[test]
    fn native_write_requires_visible_released_desktop_cursor() {
        let mut window = Window {
            focused: true,
            ..Default::default()
        };
        let mut cursor = CursorOptions::default();
        assert!(desktop_available(&window, &cursor, false));
        assert!(!desktop_available(&window, &cursor, true));
        cursor.grab_mode = CursorGrabMode::Locked;
        assert!(!desktop_available(&window, &cursor, false));
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = false;
        assert!(!desktop_available(&window, &cursor, false));
        cursor.visible = true;
        window.focused = false;
        assert!(!desktop_available(&window, &cursor, false));
    }
}
