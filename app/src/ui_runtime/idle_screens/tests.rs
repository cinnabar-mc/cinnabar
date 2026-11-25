use super::*;
use bevy::{
    input::{
        ButtonInput,
        keyboard::{KeyCode, KeyboardFocusLost, KeyboardInput},
        mouse::MouseButton,
        touch::Touches,
    },
    prelude::{App, IntoScheduleConfigs, ResMut, Resource, Update},
};

#[derive(Resource, Default)]
struct Runs(u32);

fn count(mut runs: ResMut<Runs>) {
    runs.0 += 1;
}

/// A gameplay frame with every raw input resource and one counting reader.
fn app() -> App {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<Runs>()
        .add_message::<KeyboardInput>()
        .add_message::<KeyboardFocusLost>()
        .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .configure_sets(Update, IdleScreenReaders.run_if(screen_readers_due))
        .add_systems(Update, count.in_set(IdleScreenReaders));
    app
}

fn runs(app: &App) -> u32 {
    app.world().resource::<Runs>().0
}

/// Quiet gameplay frames skip the readers; input runs them for its frame and the next, and an
/// open or requested editor keeps them running.
#[test]
fn screen_readers_settle_once_after_input_then_skip() {
    let mut app = app();
    app.update();
    assert_eq!(runs(&app), 0, "a quiet gameplay frame skips them");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyE);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    assert_eq!(runs(&app), 1, "a frame with input runs them");
    app.update();
    assert_eq!(runs(&app), 2, "the frame after still runs them to settle");
    app.update();
    assert_eq!(runs(&app), 2, "then they skip again");

    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .command_block_mut()
        .request([0, 64, 0]);
    app.update();
    app.update();
    assert_eq!(runs(&app), 4, "a waiting command block request runs them");
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .command_block_mut()
        .take_request();
    app.update();
    assert_eq!(runs(&app), 5, "the frame after the request settles");
    app.update();
    assert_eq!(runs(&app), 5);

    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .emotes_mut()
        .open();
    app.update();
    assert_eq!(runs(&app), 6, "an open emote wheel runs them");
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .emotes_mut()
        .close();
    app.update();
    app.update();
    assert_eq!(runs(&app), 7, "they settle once after it closes, then skip");
}
