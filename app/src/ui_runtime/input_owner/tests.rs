//! One press reaches one screen: whichever owned the frame, however the screens stack.

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseWheel},
    },
    prelude::{
        App, ButtonInput, Entity, IntoScheduleConfigs, KeyCode, Local, MouseButton, ResMut, Time,
        Update, Window,
    },
    time::Real,
    window::{CursorOptions, PrimaryWindow, WindowEvent},
};
use client_shell::ui_resources::{FrameInputOwner, UiPresentationResource, UiRuntimeResource};
use client_ui::{
    test_support::mini_engine_presentation,
    ui_runtime::{SequencedUiEvent, UiRuntime, scene_stack::Scene, sign_editor::SignEdit},
};
use protocol::{ddui::ScriptedScreen, test_support::ddui};

use super::{InputFrame, decide_input_owner};
use crate::ui_runtime::{
    data_driven::drive_data_driven_input, drive_chat_keyboard_input, drive_inventory_ui_actions,
    drive_server_form_input, drive_sign_editor,
};

/// A screen a test puts in the stack.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Screen {
    Inventory,
    Chat,
    Sign,
    Form,
    DataDriven,
}

/// Puts `screen` up, as whatever would open it leaves the runtime.
fn open(screen: Screen, runtime: &mut UiRuntime, player: &mut player_state::PlayerState) {
    match screen {
        Screen::Inventory => {
            runtime.toggle_inventory(player, None);
        }
        Screen::Chat => {
            runtime.open_chat(player);
        }
        Screen::Sign => {
            runtime
                .sign_editor_mut()
                .open(SignEdit::new([0, 64, 0], true, Default::default()));
        }
        Screen::Form => client_ui::test_support::pack_harness::admit_action_form(
            runtime,
            player,
            "Shop",
            &["A"],
        ),
        Screen::DataDriven => {
            let events = ddui::show_events(
                ScriptedScreen::CustomForm,
                4,
                7,
                ddui::custom_form_property(),
            );
            for (sequence, event) in (100..).zip(events) {
                runtime
                    .apply(
                        player,
                        SequencedUiEvent {
                            session_id: runtime.session_id(),
                            fifo_sequence: sequence,
                            local_millis: sequence * 10,
                            server_tick: None,
                            event,
                        },
                    )
                    .unwrap();
            }
        }
    }
}

/// The screen input systems in production order, with `screens` up. Requests go first because the
/// runtime answers one busy under a showing menu, and the stack's order doesn't depend on it.
fn app_with(screens: &[Screen]) -> (App, Entity) {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    let is_request = |screen: &&Screen| matches!(screen, Screen::Form | Screen::DataDriven);
    let requests = screens.iter().filter(is_request);
    let opened = screens.iter().filter(|screen| !is_request(screen));
    for screen in requests.chain(opened) {
        open(*screen, &mut runtime, &mut player);
    }
    let (network, packets) = client_runtime::network::NetworkHandle::<
        crate::runtime::network::resource_packs::PackApplication,
    >::stub_capturing_packets();
    // Its packets are unread, and the capture stays alive so a screen's answer can be sent.
    Box::leak(Box::new(packets));
    let mut app = App::new();
    app.insert_resource(network)
        .add_message::<KeyboardInput>()
        .add_message::<WindowEvent>()
        .add_message::<MouseButtonInput>()
        .add_message::<MouseWheel>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<FrameInputOwner>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(mini_engine_presentation()))
        .add_systems(
            Update,
            (
                decide_input_owner,
                drive_sign_editor,
                drive_server_form_input,
                drive_data_driven_input,
                drive_chat_keyboard_input,
                drive_inventory_ui_actions,
            )
                .chain(),
        );
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

/// Delivers a key press the way the window does, to both of the streams screens read.
fn press(app: &mut App, window: Entity, key_code: KeyCode, text: Option<&str>) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key_code);
    let event = KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window,
    };
    app.world_mut()
        .write_message(WindowEvent::KeyboardInput(event.clone()));
    app.world_mut().write_message(event);
}

fn is_open(app: &App, screen: Screen) -> bool {
    let runtime = app.world().resource::<UiRuntimeResource>();
    match screen {
        Screen::Inventory => runtime.inventory_open(),
        Screen::Chat => runtime.chat_focused(),
        Screen::Sign => runtime.sign_editor().is_open(),
        Screen::Form => runtime.server_forms().active().is_some(),
        Screen::DataDriven => runtime.data_driven_ui().active().is_some(),
    }
}

/// What the sign and the chat box hold.
fn typed(app: &App) -> (String, String) {
    let runtime = app.world().resource::<UiRuntimeResource>();
    (
        runtime
            .sign_editor()
            .active()
            .map_or(String::new(), SignEdit::text),
        runtime.chat_editor().as_str().to_owned(),
    )
}

/// A screen with one over it.
const STACKS: [(Screen, Screen); 6] = [
    (Screen::Inventory, Screen::Sign),
    (Screen::Inventory, Screen::Form),
    (Screen::Chat, Screen::Sign),
    (Screen::Chat, Screen::Form),
    (Screen::Sign, Screen::Form),
    (Screen::Sign, Screen::DataDriven),
];

#[test]
fn a_key_that_closes_the_top_screen_leaves_the_one_beneath_it_open() {
    for (lower, upper) in STACKS {
        let (mut app, window) = app_with(&[lower, upper]);
        assert!(is_open(&app, lower), "{lower:?} under {upper:?}");
        press(&mut app, window, KeyCode::Escape, None);
        app.update();
        assert!(!is_open(&app, upper), "{lower:?} under {upper:?}");
        assert!(is_open(&app, lower), "{lower:?} under {upper:?}");
    }
}

#[test]
fn typing_reaches_only_the_top_screen() {
    for (lower, upper) in STACKS {
        let (mut app, window) = app_with(&[lower, upper]);
        press(&mut app, window, KeyCode::KeyA, Some("a"));
        app.update();
        let (sign, chat) = typed(&app);
        let expected = |screen| if upper == screen { "a" } else { "" };
        assert_eq!(sign, expected(Screen::Sign), "{lower:?} under {upper:?}");
        assert_eq!(chat, expected(Screen::Chat), "{lower:?} under {upper:?}");
        assert!(is_open(&app, lower), "{lower:?} under {upper:?}");
    }
}

/// With nothing beneath it, what closes a screen stops there too: it opens nothing.
#[test]
fn the_keys_of_a_screens_closing_frame_open_nothing() {
    let (mut app, window) = app_with(&[Screen::Sign]);
    press(&mut app, window, KeyCode::Escape, None);
    press(&mut app, window, KeyCode::KeyE, Some("e"));
    press(&mut app, window, KeyCode::KeyT, Some("t"));
    app.update();
    assert!(!is_open(&app, Screen::Sign));
    assert!(!is_open(&app, Screen::Inventory));
    assert!(!is_open(&app, Screen::Chat));
    // The next frame's keys are the game's again.
    press(&mut app, window, KeyCode::KeyT, Some("t"));
    app.update();
    assert!(is_open(&app, Screen::Chat));
}

/// A screen that opens during a frame reads from the next one, and the rest of the frame's keys
/// reach neither it nor what it covers.
#[test]
fn a_screen_opened_during_a_frame_reads_from_the_next_one() {
    fn open_sign(mut runtime: ResMut<UiRuntimeResource>, mut once: Local<bool>) {
        if !std::mem::replace(&mut once, true) {
            runtime
                .sign_editor_mut()
                .open(SignEdit::new([0, 64, 0], true, Default::default()));
        }
    }
    let (mut app, window) = app_with(&[]);
    app.add_systems(
        Update,
        open_sign
            .after(decide_input_owner)
            .before(drive_sign_editor),
    );
    press(&mut app, window, KeyCode::KeyT, Some("t"));
    app.update();
    assert!(!is_open(&app, Screen::Chat));
    assert_eq!(typed(&app), (String::new(), String::new()));
    press(&mut app, window, KeyCode::KeyB, Some("b"));
    app.update();
    assert_eq!(typed(&app), ("b".to_owned(), String::new()));
}

#[test]
fn a_frame_is_read_only_by_what_owned_it_throughout() {
    let frame = |began, top| InputFrame { began, top };
    let sign = |scene| scene == Scene::SignEditor;
    assert!(frame(Scene::SignEditor, Scene::SignEditor).reads(sign));
    // Opened this frame, or closed this frame beneath a reader: neither reads.
    assert!(!frame(Scene::Gameplay, Scene::SignEditor).reads(sign));
    assert!(!frame(Scene::SignEditor, Scene::Gameplay).reads(|scene| scene == Scene::Gameplay));
    assert!(frame(Scene::Container, Scene::ServerForm).covers(Scene::SignEditor));
    assert!(!frame(Scene::Container, Scene::Container).covers(Scene::SignEditor));
}
