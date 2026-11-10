//! When OreUI controls sound through the live menu input system.

use bevy::{
    input::{
        keyboard::Key,
        touch::{TouchInput, TouchPhase, touch_screen_input_system},
    },
    prelude::*,
    window::WindowResolution,
};
use client_ui::sound_requests::UI_CLICK;
use client_ui::ui_runtime::UiRuntime;
use client_ui::ui_runtime::presentation::forms::CapturedMenuSounds;
use launcher::menu::settings_options::SettingId;
use launcher::menu::{MenuAction, MenuScreen};
use ui::DpiScale;

use super::*;

const PHYSICAL: [u32; 2] = [1920, 1080];

/// The menu input system over a painted OreUI Settings section holding a toggle.
fn settings_fixture() -> (App, Entity, MenuAction, Vec2, CapturedMenuSounds) {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.set_named_option(SettingId::ScreenAnimations, 0);
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.activate(MenuAction::SettingsSection(
        launcher::menu::settings_sections::SettingSection::Accessibility
            .definition()
            .index,
    ));
    let value = menu.settings_options.value(SettingId::CameraShake);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let sounds = presentation.capture_menu_press_sounds();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .add_message::<TouchInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(menu)
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(PreUpdate, touch_screen_input_system)
        .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                resolution: WindowResolution::new(PHYSICAL[0], PHYSICAL[1]),
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
            presentation.set_menu_view(Some(view));
            presentation
                .build(
                    world.resource::<client_state::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    PHYSICAL,
                    DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        });
    let action = MenuAction::SettingsOption(SettingId::CameraShake.index() as u16, 1 - value);
    let bounds = app
        .world()
        .resource::<UiPresentationResource>()
        .menu_action_bounds(action)
        .unwrap();
    let centre = Vec2::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    );
    (app, window, action, centre, sounds)
}

/// Clicks this fixture's presentation played since the last call.
fn clicks(sounds: &CapturedMenuSounds) -> usize {
    let played = std::mem::take(&mut *sounds.lock().unwrap());
    played.iter().filter(|name| *name == UI_CLICK).count()
}

/// Moves the mouse to `position` with a left button edge, then runs one frame.
fn mouse(app: &mut App, window: Entity, position: Vec2, state: ButtonState) {
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .set_cursor_position(Some(position));
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state,
        window,
    });
    let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    match state {
        ButtonState::Pressed => buttons.press(MouseButton::Left),
        ButtonState::Released => buttons.release(MouseButton::Left),
    }
    app.update();
}

fn touch(app: &mut App, window: Entity, position: Vec2, phase: TouchPhase) {
    app.world_mut().write_message(TouchInput {
        phase,
        position,
        window,
        force: None,
        id: 7,
    });
    app.update();
}

fn camera_shake(app: &App) -> i32 {
    app.world()
        .resource::<MenuRuntime>()
        .settings_options
        .value(SettingId::CameraShake)
}

/// Settings controls act on release, but vanilla sounds the press as the button goes down.
#[test]
fn a_mouse_press_clicks_going_down_and_not_on_release() {
    let (mut app, window, _, centre, sounds) = settings_fixture();
    let before = camera_shake(&app);
    mouse(&mut app, window, centre, ButtonState::Pressed);
    assert_eq!(clicks(&sounds), 1);
    assert_eq!(camera_shake(&app), before, "the press alone must not act");
    mouse(&mut app, window, centre, ButtonState::Released);
    assert_eq!(clicks(&sounds), 0);
    assert_ne!(camera_shake(&app), before);
}

/// A press that slides off still clicks; vanilla sounds it before knowing the outcome.
#[test]
fn a_mouse_press_released_elsewhere_clicks_once_without_acting() {
    let (mut app, window, _, centre, sounds) = settings_fixture();
    let before = camera_shake(&app);
    mouse(&mut app, window, centre, ButtonState::Pressed);
    mouse(&mut app, window, Vec2::ZERO, ButtonState::Released);
    assert_eq!(clicks(&sounds), 1);
    assert_eq!(camera_shake(&app), before);
}

#[test]
fn a_press_on_empty_space_is_silent() {
    let (mut app, window, _, _, sounds) = settings_fixture();
    let empty = (0..PHYSICAL[1])
        .step_by(20)
        .flat_map(|y| (0..PHYSICAL[0]).step_by(20).map(move |x| (x, y)))
        .map(|(x, y)| Vec2::new(x as f32, y as f32))
        .find(|point| {
            let point = UiPoint::new(point.x, point.y).unwrap();
            app.world()
                .resource::<UiPresentationResource>()
                .hit_test_menu(point)
                .is_none()
        })
        .expect("the fixture screen has space outside its controls");
    mouse(&mut app, window, empty, ButtonState::Pressed);
    mouse(&mut app, window, empty, ButtonState::Released);
    assert_eq!(clicks(&sounds), 0);
}

#[test]
fn keyboard_selection_clicks_once() {
    let (mut app, window, action, _, sounds) = settings_fixture();
    // The first frame publishes the painted controls as focus targets.
    app.update();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .focus_pointer(action);
    let before = camera_shake(&app);
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
    assert_eq!(clicks(&sounds), 1);
    assert_ne!(camera_shake(&app), before);
}

/// However long the finger rests, a tap sounds once and never again on release.
#[test]
fn a_touch_tap_clicks_once() {
    let (mut app, window, _, centre, sounds) = settings_fixture();
    touch(&mut app, window, centre, TouchPhase::Started);
    touch(&mut app, window, centre, TouchPhase::Ended);
    assert_eq!(clicks(&sounds), 1);
}

/// A swipe that leaves before the resting delay never becomes a press.
#[test]
fn a_touch_swipe_is_silent() {
    let (mut app, window, _, centre, sounds) = settings_fixture();
    touch(&mut app, window, centre, TouchPhase::Started);
    let away = centre + Vec2::new(0.0, 80.0);
    touch(&mut app, window, away, TouchPhase::Moved);
    touch(&mut app, window, away, TouchPhase::Ended);
    assert_eq!(clicks(&sounds), 0);
}
