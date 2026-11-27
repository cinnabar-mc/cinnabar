use super::*;
use bevy::{
    input::{ButtonState, gamepad::Gamepad, keyboard::Key},
    prelude::{App, Entity, IntoScheduleConfigs, ResMut, Resource, Update},
};

#[derive(Resource, Default)]
struct Runs(u32);

fn count(mut runs: ResMut<Runs>) {
    runs.0 += 1;
}

/// An app with every raw input resource and one counting reader in [`RawInputReaders`].
fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<Runs>()
        .add_message::<KeyboardInput>()
        .add_message::<KeyboardFocusLost>()
        .configure_sets(Update, RawInputReaders.run_if(raw_input_activity))
        .add_systems(Update, count.in_set(RawInputReaders));
    app
}

fn runs(app: &App) -> u32 {
    app.world().resource::<Runs>().0
}

/// Readers skip a frame without input and run on every frame that carries some.
#[test]
fn readers_run_only_on_frames_with_raw_input() {
    let mut app = app();
    app.update();
    assert_eq!(runs(&app), 0, "a frame without input skips the readers");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyQ);
    app.update();
    assert_eq!(runs(&app), 1, "a just-pressed key runs them");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(runs(&app), 1, "a key held without change does not");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyQ);
    app.update();
    assert_eq!(runs(&app), 2, "its release does");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();

    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyE,
        logical_key: Key::Character("e".into()),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();
    assert_eq!(runs(&app), 3, "a keyboard message runs them");
    let settled = settle(&mut app);

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(runs(&app), settled + 1, "a mouse press runs them");
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();

    let before = runs(&app);
    app.world_mut().write_message(KeyboardFocusLost);
    app.update();
    assert_eq!(runs(&app), before + 1, "losing keyboard focus runs them");
    let settled = settle(&mut app);

    let pad = app.world_mut().spawn(Gamepad::default()).id();
    app.update();
    app.update();
    assert_eq!(
        runs(&app),
        settled + 2,
        "a connected gamepad runs them every frame"
    );
    app.world_mut().despawn(pad);
    app.update();
    assert_eq!(runs(&app), settled + 2);
}

/// Updates until buffered messages expire, then checks that a quiet frame skips the readers
/// again, returning the run count.
fn settle(app: &mut App) -> u32 {
    for _ in 0..3 {
        app.update();
    }
    let settled = runs(app);
    app.update();
    assert_eq!(runs(app), settled, "a quiet frame skips the readers again");
    settled
}

/// A harness without the input resources runs every reader, as it did before the condition.
#[test]
fn missing_input_resources_count_as_input() {
    let mut app = App::new();
    app.init_resource::<Runs>()
        .configure_sets(Update, RawInputReaders.run_if(raw_input_activity))
        .add_systems(Update, count.in_set(RawInputReaders));
    app.update();
    assert_eq!(runs(&app), 1);
}
