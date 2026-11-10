//! Desktop fullscreen state shared by the video settings and F11.
//! The pinned Bedrock `ui/settings_sections/general_section.json` control is
//! `full_screen`; entering fullscreen uses the current monitor without changing
//! its display mode.

use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::{DetectChangesMut, Entity, Local, MessageReader, Mut, Query, Res, ResMut, With},
    window::{MonitorSelection, PrimaryWindow, Window, WindowMode},
};
use launcher::menu::settings_options::BindingId;

use {crate::menu::MenuRuntime, client_state::RuntimeSettings};

mod native;
pub(crate) mod recovery;
pub use native::NativeFullscreen;
pub use recovery::FullscreenTransition;
use render_scene::startup_presentation::StartupPresentation;

/// Read before menu input clears its keyboard state. OS key repeats must not
/// flip fullscreen again while F11 is held, including on a visible menu.
pub fn toggle_fullscreen_hotkey(
    mut keyboard: MessageReader<KeyboardInput>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
    mut transition: Option<ResMut<FullscreenTransition>>,
) {
    let Ok((entity, mut window)) = windows.single_mut() else {
        keyboard.clear();
        return;
    };
    if !window.visible {
        keyboard.clear();
        return;
    }
    let toggles = keyboard
        .read()
        .filter(|input| {
            input.window == entity
                && crate::menu::settings_options::control_bindings::binding_key(
                    Some(&menu),
                    BindingId::Fullscreen,
                    input.key_code,
                )
                && input.state == ButtonState::Pressed
                && !input.repeat
        })
        .count();
    if !window.focused || toggles % 2 == 0 {
        return;
    }
    let fullscreen = !menu.take_fullscreen_change().unwrap_or_else(|| {
        transition
            .as_ref()
            .and_then(|transition| transition.requested())
            .unwrap_or_else(|| is_fullscreen(&window))
    });
    set_fullscreen(&mut window, fullscreen, transition.as_deref_mut());
    record_fullscreen(&mut settings, fullscreen);
    menu.sync_fullscreen(fullscreen);
}

/// Apply menu presses and retained settings before the UI publishes its next
/// frame, so the checkbox and the native primary window always agree.
pub fn apply_runtime_fullscreen_setting(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
    mut observed_generation: Local<u64>,
    mut transition: Option<ResMut<FullscreenTransition>>,
    feedback: Option<Res<StartupPresentation>>,
    native: Option<Res<NativeFullscreen>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    // A hidden capture window keeps its requested size and leaves the saved preference alone.
    if !window.visible {
        return;
    }
    if let Some(fullscreen) = menu.take_fullscreen_change() {
        set_fullscreen(&mut window, fullscreen, transition.as_deref_mut());
        record_fullscreen(&mut settings, fullscreen);
    } else {
        let (generation, user_settings) = settings.user_settings_update();
        if generation > *observed_generation {
            set_fullscreen(
                &mut window,
                user_settings.video.fullscreen,
                transition.as_deref_mut(),
            );
        }
    }
    if let Some(transition) = transition.as_deref_mut() {
        update_transition(
            &mut window,
            transition,
            feedback.as_deref(),
            native.and_then(|native| native.0),
        );
    }
    let fullscreen = transition
        .as_ref()
        .and_then(|transition| transition.requested())
        .unwrap_or_else(|| is_fullscreen(&window));
    record_fullscreen(&mut settings, fullscreen);
    *observed_generation = settings.user_settings_update().0;
    menu.sync_fullscreen(fullscreen);
}

/// Advances a guarded mode change. This runs every frame and rarely writes, and a changed window
/// costs a full sync with the native one, so only a new mode marks the window changed.
fn update_transition(
    window: &mut Mut<Window>,
    transition: &mut FullscreenTransition,
    feedback: Option<&StartupPresentation>,
    native: Option<bool>,
) {
    let mode = window.mode;
    transition.update(window.bypass_change_detection(), feedback, native);
    if window.mode != mode {
        window.set_changed();
    }
}

/// Reports the primary window's requested native mode.
fn is_fullscreen(window: &Window) -> bool {
    window.mode != WindowMode::Windowed
}

/// The primary window's mode for a fullscreen preference. Where the platform only shows the
/// app full screen, the window always fills its one display, so a windowed size never applies.
pub fn window_mode(fullscreen: bool) -> WindowMode {
    if launcher::FULLSCREEN_ONLY {
        WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
    } else if fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    }
}

/// Queues guarded native entry, or applies it directly for adapters without recovery.
pub(crate) fn set_fullscreen(
    window: &mut Window,
    fullscreen: bool,
    transition: Option<&mut FullscreenTransition>,
) {
    if let Some(transition) = transition {
        transition.request(fullscreen);
        return;
    }
    if fullscreen != is_fullscreen(window) {
        window.mode = window_mode(fullscreen);
    }
}

/// Retains the requested mode without changing any other user settings.
fn record_fullscreen(settings: &mut RuntimeSettings, fullscreen: bool) {
    settings.set_fullscreen(fullscreen);
}

#[cfg(all(test, not(target_os = "ios")))]
mod tests {
    use super::*;
    use bevy::prelude::{App, Changed, IntoScheduleConfigs, Resource, Update};

    /// Counts updates whose primary window was marked changed, which triggers a native sync.
    #[derive(Default, Resource)]
    struct WindowChanges(u32);

    fn advance_transition(
        mut windows: Query<&mut Window, With<PrimaryWindow>>,
        mut transition: ResMut<FullscreenTransition>,
    ) {
        update_transition(
            &mut windows.single_mut().unwrap(),
            &mut transition,
            None,
            None,
        );
    }

    fn count_window_changes(
        changed: Query<(), (Changed<Window>, With<PrimaryWindow>)>,
        mut changes: ResMut<WindowChanges>,
    ) {
        changes.0 += u32::from(!changed.is_empty());
    }

    #[test]
    fn only_a_new_mode_from_a_transition_marks_the_window_changed() {
        let mut app = App::new();
        app.init_resource::<WindowChanges>()
            .init_resource::<FullscreenTransition>()
            .add_systems(Update, (advance_transition, count_window_changes).chain());
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        app.world_mut().resource_mut::<WindowChanges>().0 = 0;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<WindowChanges>().0, 0);

        app.world_mut()
            .resource_mut::<FullscreenTransition>()
            .request(true);
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().mode,
            window_mode(true)
        );
        assert_eq!(app.world().resource::<WindowChanges>().0, 1);
    }

    #[test]
    fn desktop_and_android_window_modes_follow_the_fullscreen_preference() {
        assert_eq!(window_mode(false), WindowMode::Windowed);
        assert_eq!(
            window_mode(true),
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        );
    }
}
