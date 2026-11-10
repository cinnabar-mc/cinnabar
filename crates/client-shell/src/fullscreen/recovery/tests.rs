use super::*;
use bevy::{
    input::keyboard::{Key, NativeKey},
    prelude::{Entity, KeyCode, MinimalPlugins, Update},
};
use launcher::menu::MenuAction;
use launcher_host::video_settings::{self, SavedVideoSettings};

/// Builds the real fullscreen adapters and recovery owner without a native graphics driver.
fn app(layout: launcher::install_layout::InstallLayout) -> (App, Entity) {
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Fullscreen");
    let menu = MenuRuntime::new_with_layout(true, Some(2), "Fullscreen".into(), layout, skin);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .init_resource::<RuntimeSettings>()
        .init_resource::<StartupPresentation>()
        .insert_resource(menu)
        .add_systems(
            Update,
            (
                toggle_fullscreen_hotkey,
                apply_runtime_fullscreen_setting,
                crate::settings_runtime::apply_window_settings,
            )
                .chain(),
        );
    configure(&mut app);
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    (app, window)
}

/// Advances the adapter until entry has passed its durable-write barrier.
fn enter(app: &mut App, window: Entity) {
    test_time::eventually("journaled fullscreen entry", || {
        app.update();
        is_fullscreen(app.world().get::<Window>(window).unwrap())
    });
}

/// Settles the platform into fullscreen, then starts the presentation that must follow it.
fn settle(app: &mut App) {
    app.world_mut().resource_mut::<NativeFullscreen>().0 = Some(true);
    app.update();
}

/// Completes the exact mode's render feedback and drains the asynchronous journal removal.
fn present(app: &mut App) {
    settle(app);
    let feedback = app.world().resource::<StartupPresentation>();
    let epoch = feedback.pending_mode_change().unwrap();
    feedback.record_mode_frame(epoch);
    test_time::eventually("fullscreen journal cleared after presentation", || {
        app.update();
        let transition = app.world().resource::<FullscreenTransition>();
        !transition.armed && transition.saving.is_none()
    });
}

#[test]
fn a_crash_during_saved_fullscreen_entry_starts_the_next_launch_windowed() {
    let layout = launcher::test_support::scratch("fullscreen-crash");
    video_settings::save(
        &layout.user_config_root,
        SavedVideoSettings {
            fullscreen: true,
            gui_scale_offset: 1,
        },
    )
    .unwrap();
    let (mut first, window) = app(layout.clone());
    // An already-completed startup frame must not count as fullscreen success.
    first
        .world()
        .resource::<StartupPresentation>()
        .record_completed_frame();
    enter(&mut first, window);
    assert!(first.world().resource::<FullscreenTransition>().armed);
    drop(first);
    let (mut next, window) = app(layout.clone());
    next.update();
    assert!(!is_fullscreen(next.world().get::<Window>(window).unwrap()));
    assert!(
        !next
            .world()
            .resource::<RuntimeSettings>()
            .user_settings_update()
            .1
            .video
            .fullscreen
    );
    assert_eq!(
        video_settings::load(&layout.user_config_root).unwrap(),
        SavedVideoSettings {
            fullscreen: false,
            gui_scale_offset: 1
        }
    );
}

#[test]
fn an_unrelated_crash_after_success_keeps_the_saved_fullscreen_preference() {
    let layout = launcher::test_support::scratch("fullscreen-success");
    let saved = SavedVideoSettings {
        fullscreen: true,
        gui_scale_offset: 1,
    };
    video_settings::save(&layout.user_config_root, saved).unwrap();
    let (mut first, window) = app(layout.clone());
    enter(&mut first, window);
    present(&mut first);
    drop(first);
    assert_eq!(
        video_settings::load(&layout.user_config_root).unwrap(),
        saved
    );
    let (mut next, window) = app(layout);
    enter(&mut next, window);
}

#[test]
fn deliberate_close_during_entry_does_not_overwrite_fullscreen() {
    let layout = launcher::test_support::scratch("fullscreen-close");
    let saved = SavedVideoSettings {
        fullscreen: true,
        gui_scale_offset: 0,
    };
    video_settings::save(&layout.user_config_root, saved).unwrap();
    let (mut first, window) = app(layout.clone());
    enter(&mut first, window);
    first.world_mut().write_message(AppExit::Success);
    first.update();
    drop(first);
    assert_eq!(
        video_settings::load(&layout.user_config_root).unwrap(),
        saved
    );
    let (mut next, window) = app(layout);
    enter(&mut next, window);
}

#[test]
fn hotkey_and_menu_entry_are_guarded_after_renderer_startup_succeeded() {
    for hotkey in [true, false] {
        let layout = launcher::test_support::scratch("fullscreen-runtime");
        let (mut app, window) = app(layout.clone());
        app.update();
        app.world()
            .resource::<StartupPresentation>()
            .record_completed_frame();
        if hotkey {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::F11,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        } else {
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .activate(MenuAction::SettingsFullscreen(true));
        }
        enter(&mut app, window);
        assert!(app.world().resource::<FullscreenTransition>().armed);
        assert!(FullscreenGuard::open(&layout.user_config_root).is_err());
        drop(app);
        assert!(
            FullscreenGuard::open(&layout.user_config_root)
                .unwrap()
                .recovered
        );
    }
}

#[test]
fn cancelling_pending_entry_leaves_windowed_and_does_not_reenter() {
    let layout = launcher::test_support::scratch("fullscreen-cancel");
    let (mut app, window) = app(layout.clone());
    app.update();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(true));
    app.update(); // Entry can only start a journal write on this update.
    assert!(!is_fullscreen(app.world().get::<Window>(window).unwrap()));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsFullscreen(false));
    test_time::eventually("cancelled fullscreen entry", || {
        app.update();
        let transition = app.world().resource::<FullscreenTransition>();
        transition.requested.is_none() && !transition.armed && transition.saving.is_none()
    });
    assert!(!is_fullscreen(app.world().get::<Window>(window).unwrap()));
    drop(app);
    assert!(
        !FullscreenGuard::open(&layout.user_config_root)
            .unwrap()
            .recovered
    );
}

#[test]
fn a_settings_reload_cannot_bypass_the_durable_entry_barrier() {
    let layout = launcher::test_support::scratch("fullscreen-reload");
    let (mut app, window) = app(layout);
    app.update();
    let mut user = app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1
        .clone();
    user.video.fullscreen = true;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(user);
    app.update();
    assert!(!is_fullscreen(app.world().get::<Window>(window).unwrap()));
    assert!(
        app.world()
            .resource::<FullscreenTransition>()
            .saving
            .is_some()
    );
    enter(&mut app, window);
}

#[test]
fn cleanup_worker_start_failure_preserves_fullscreen_after_crash_or_close() {
    for orderly in [false, true] {
        let layout = launcher::test_support::scratch("fullscreen-worker-failure");
        let saved = SavedVideoSettings {
            fullscreen: true,
            gui_scale_offset: 1,
        };
        video_settings::save(&layout.user_config_root, saved).unwrap();
        let (mut app, window) = app(layout.clone());
        enter(&mut app, window);
        settle(&mut app);
        let feedback = app.world().resource::<StartupPresentation>();
        feedback.record_mode_frame(feedback.pending_mode_change().unwrap());
        app.world_mut()
            .resource_mut::<FullscreenTransition>()
            .save_with(false, |_, _| {
                Err(std::io::Error::other("thread creation failed"))
            });
        assert!(!app.world().resource::<FullscreenTransition>().armed);
        if orderly {
            app.world_mut().write_message(AppExit::Success);
            app.update();
        }
        drop(app);
        assert!(
            !FullscreenGuard::open(&layout.user_config_root)
                .unwrap()
                .recovered
        );
        assert_eq!(
            video_settings::load(&layout.user_config_root).unwrap(),
            saved
        );
    }
}

#[test]
fn a_presentation_before_the_platform_settles_cannot_clear_entry_attribution() {
    let layout = launcher::test_support::scratch("fullscreen-unsettled");
    video_settings::save(
        &layout.user_config_root,
        SavedVideoSettings {
            fullscreen: true,
            gui_scale_offset: 1,
        },
    )
    .unwrap();
    let (mut first, window) = app(layout.clone());
    enter(&mut first, window);
    // The renderer keeps presenting while the window is still windowed or animating.
    for native in [Some(false), None, Some(false), None] {
        first.world_mut().resource_mut::<NativeFullscreen>().0 = native;
        let feedback = first.world().resource::<StartupPresentation>();
        feedback.record_completed_frame();
        if let Some(epoch) = feedback.pending_mode_change() {
            feedback.record_mode_frame(epoch);
        }
        first.update();
    }
    let transition = first.world().resource::<FullscreenTransition>();
    assert!(transition.armed && transition.saving.is_none());
    drop(first);
    let (mut next, window) = app(layout.clone());
    next.update();
    assert!(!is_fullscreen(next.world().get::<Window>(window).unwrap()));
    assert!(
        !video_settings::load(&layout.user_config_root)
            .unwrap()
            .fullscreen
    );
}

#[test]
fn a_client_launched_during_another_transition_follows_it_once_released() {
    for crashed in [false, true] {
        let layout = launcher::test_support::scratch("fullscreen-contended");
        video_settings::save(
            &layout.user_config_root,
            SavedVideoSettings {
                fullscreen: true,
                gui_scale_offset: 1,
            },
        )
        .unwrap();
        let mut other = FullscreenGuard::open(&layout.user_config_root).unwrap();
        other.begin().unwrap();
        let (mut second, window) = app(layout.clone());
        second.update();
        second.update();
        assert!(!is_fullscreen(
            second.world().get::<Window>(window).unwrap()
        ));
        if crashed {
            drop(other);
            test_time::eventually("recovery opened after the other client crashed", || {
                second.update();
                let transition = second.world().resource::<FullscreenTransition>();
                transition.guard.is_some() && transition.opening.is_none()
            });
            second.update();
            assert!(!is_fullscreen(
                second.world().get::<Window>(window).unwrap()
            ));
            assert!(
                !second
                    .world()
                    .resource::<RuntimeSettings>()
                    .user_settings_update()
                    .1
                    .video
                    .fullscreen
            );
            assert!(
                !video_settings::load(&layout.user_config_root)
                    .unwrap()
                    .fullscreen
            );
        } else {
            other.clear().unwrap();
            drop(other);
            enter(&mut second, window);
            assert!(second.world().resource::<FullscreenTransition>().armed);
        }
    }
}
