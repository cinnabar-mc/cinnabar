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
fn shell_controls_syncs_presentation_from_effective_input_mode() {
    for mode in [InputMode::Touch, InputMode::KeyboardMouse] {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(false, 2, "Controls".into());
        menu.set_touch_available(true);

        app.init_resource::<GameplayScreenControls>()
            .insert_resource(client_state::EffectiveGameplayInputMode(mode))
            .insert_resource(UiPresentationResource(fixture_presentation()))
            .insert_resource(menu);

        app.world_mut()
            .run_system_once(publish_gameplay_screen_controls)
            .unwrap();

        let pres = app.world().resource::<UiPresentationResource>();
        assert_eq!(pres.touch_controls_enabled(), mode == InputMode::Touch);
    }
}

#[test]
fn shell_controls_disables_touch_presentation_when_touch_unavailable() {
    let mut app = App::new();
    let mut menu = MenuRuntime::new(false, 2, "Controls".into());
    menu.set_touch_available(false);

    app.init_resource::<GameplayScreenControls>()
        .insert_resource(UiPresentationResource(fixture_presentation()))
        .insert_resource(menu);

    app.world_mut()
        .run_system_once(publish_gameplay_screen_controls)
        .unwrap();

    let pres = app.world().resource::<UiPresentationResource>();
    assert!(!pres.touch_controls_enabled());
}
