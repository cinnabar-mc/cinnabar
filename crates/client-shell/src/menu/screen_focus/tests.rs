use super::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use client_ui::ui_runtime::UiRuntime;

/// Publishes the shell observation from the fixture's current screen owners.
fn publish(app: &mut App) -> ScreenFocus {
    app.world_mut()
        .run_system_once(publish_screen_focus)
        .unwrap();
    *app.world().resource::<ScreenFocus>()
}

#[test]
fn focus_tracks_menu_and_ui_changes_without_retaining_stale_ownership() {
    let mut app = App::new();
    app.init_resource::<ScreenFocus>()
        .insert_resource(PlayerRuntime(player_state::PlayerState::new(1)))
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(MenuRuntime::new(false, 2, "Focus".into()));
    assert!(!publish(&mut app).absorbs_input);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(true);
    let focus = publish(&mut app);
    assert!(focus.menu_visible && focus.absorbs_input);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(false);
    app.world_mut()
        .resource_scope(|world, mut player: Mut<PlayerRuntime>| {
            world
                .resource_mut::<UiRuntimeResource>()
                .open_chat(&mut player);
        });
    let focus = publish(&mut app);
    assert!(!focus.menu_visible && focus.ui_focused && focus.absorbs_input);

    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .close_chat();
    let focus = publish(&mut app);
    assert!(!focus.menu_visible && !focus.ui_focused && !focus.absorbs_input);
}

#[test]
fn shell_controls_publish_pick_binding_and_clear_the_next_frame() {
    let mut app = App::new();
    app.init_resource::<GameplayScreenControls>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(MenuRuntime::new(false, 2, "Controls".into()));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    app.world_mut()
        .run_system_once(publish_gameplay_screen_controls)
        .unwrap();
    assert!(
        app.world()
            .resource::<GameplayScreenControls>()
            .pick_block_pressed
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .run_system_once(publish_gameplay_screen_controls)
        .unwrap();
    assert!(
        !app.world()
            .resource::<GameplayScreenControls>()
            .pick_block_pressed
    );
}

#[test]
fn shell_controls_publish_hide_hud() {
    for hidden in [false, true] {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(false, 2, "Controls".into());
        menu.set_session_option(SettingId::HideHud, hidden.then_some(1));
        app.init_resource::<GameplayScreenControls>()
            .insert_resource(menu);
        app.world_mut()
            .run_system_once(publish_gameplay_screen_controls)
            .unwrap();
        assert_eq!(
            app.world().resource::<GameplayScreenControls>().hide_hud,
            hidden
        );
    }
}

fn fixture_presentation() -> client_ui::ui_runtime::presentation::UiPresentationRuntime {
    client_ui::ui_runtime::presentation::UiPresentationRuntime::new(
        ui_engine::test_support::fixture_font(),
    )
    .unwrap()
}

#[test]
fn gameplay_input_mode_toggle_switches_between_touch_and_keyboard_mouse() {
    let mut menu = MenuRuntime::new(false, 2, "Controls".into());
    menu.set_gameplay_input_mode(InputMode::Touch);
    assert_eq!(menu.gameplay_input_mode(), InputMode::Touch);

    menu.activate(launcher::menu::MenuAction::ToggleGameplayInputMode);
    assert_eq!(menu.gameplay_input_mode(), InputMode::KeyboardMouse);

    menu.activate(launcher::menu::MenuAction::ToggleGameplayInputMode);
    assert_eq!(menu.gameplay_input_mode(), InputMode::Touch);
}

#[test]
fn gamepad_input_never_selectable_via_toggle() {
    let mut menu = MenuRuntime::new(false, 2, "Controls".into());
    menu.set_gameplay_input_mode(InputMode::GamePad);
    // set_gameplay_input_mode coerces GamePad to KeyboardMouse
    assert_eq!(menu.gameplay_input_mode(), InputMode::KeyboardMouse);

    // Even if internal state was GamePad, toggling normalizes it to Touch
    menu.gameplay_input_mode = InputMode::GamePad;
    assert_eq!(menu.toggle_gameplay_input_mode(), InputMode::Touch);
    assert_eq!(menu.toggle_gameplay_input_mode(), InputMode::KeyboardMouse);
    assert_eq!(menu.toggle_gameplay_input_mode(), InputMode::Touch);
}

#[test]
fn gameplay_input_mode_preference_survives_pause_and_resume() {
    let mut menu = MenuRuntime::new(false, 2, "Controls".into());
    menu.set_visible(true);
    menu.set_gameplay_input_mode(InputMode::KeyboardMouse);
    assert_eq!(menu.gameplay_input_mode(), InputMode::KeyboardMouse);

    // Resume from pause menu
    menu.activate(launcher::menu::MenuAction::PauseResume);
    assert!(!menu.is_visible());
    assert_eq!(menu.gameplay_input_mode(), InputMode::KeyboardMouse);

    // Reopen pause menu
    menu.set_visible(true);
    assert_eq!(menu.gameplay_input_mode(), InputMode::KeyboardMouse);

    // Toggle to Touch and resume again
    menu.activate(launcher::menu::MenuAction::ToggleGameplayInputMode);
    assert_eq!(menu.gameplay_input_mode(), InputMode::Touch);
    menu.activate(launcher::menu::MenuAction::PauseResume);
    assert!(!menu.is_visible());
    assert_eq!(menu.gameplay_input_mode(), InputMode::Touch);
}

#[test]
fn menu_view_reflects_selected_gameplay_input_mode() {
    let mut menu = MenuRuntime::new(false, 2, "Controls".into());
    menu.set_gameplay_input_mode(InputMode::Touch);
    assert_eq!(menu.view().gameplay_input_mode, InputMode::Touch);

    menu.activate(launcher::menu::MenuAction::ToggleGameplayInputMode);
    assert_eq!(menu.view().gameplay_input_mode, InputMode::KeyboardMouse);

    menu.activate(launcher::menu::MenuAction::ToggleGameplayInputMode);
    assert_eq!(menu.view().gameplay_input_mode, InputMode::Touch);
}

#[test]
fn shell_controls_publish_locked_input_mode_and_sync_presentation() {
    // 1. Touch available: publishes Some(mode) and synchronizes presentation.enable_touch_controls
    for mode in [InputMode::Touch, InputMode::KeyboardMouse] {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(false, 2, "Controls".into());
        menu.set_touch_available(true);
        menu.set_gameplay_input_mode(mode);

        app.init_resource::<GameplayScreenControls>()
            .insert_resource(UiPresentationResource(fixture_presentation()))
            .insert_resource(menu);

        app.world_mut()
            .run_system_once(publish_gameplay_screen_controls)
            .unwrap();

        let controls = *app.world().resource::<GameplayScreenControls>();
        assert_eq!(controls.locked_input_mode, Some(mode));

        let pres = app.world().resource::<UiPresentationResource>();
        assert_eq!(pres.touch_controls_enabled(), mode == InputMode::Touch);
    }

    // 2. Touch unavailable (desktop): publishes None and does not enable touch controls
    {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(false, 2, "Controls".into());
        menu.set_touch_available(false);
        menu.set_gameplay_input_mode(InputMode::Touch);

        app.init_resource::<GameplayScreenControls>()
            .insert_resource(UiPresentationResource(fixture_presentation()))
            .insert_resource(menu);

        app.world_mut()
            .run_system_once(publish_gameplay_screen_controls)
            .unwrap();

        let controls = *app.world().resource::<GameplayScreenControls>();
        assert_eq!(controls.locked_input_mode, None);

        let pres = app.world().resource::<UiPresentationResource>();
        assert!(!pres.touch_controls_enabled());
    }
}
