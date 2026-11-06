//! The Mob Effects binding's production adapter: opening, closing and frame ownership.

use super::*;
use bevy::input::keyboard::{Key, NativeKey};
use bevy::window::CursorGrabMode;
use client_shell::ui_resources::UiRuntimeResource;
use client_state::ScreenInputConsumed;
use client_ui::ui_runtime::UiRuntime;
use protocol::{ActorEffectAction, ActorEffectEvent};

struct Harness {
    app: App,
    window: Entity,
}

impl Harness {
    /// A focused, captured window with the local player published and, if `effect`, one
    /// infinite Speed effect applied.
    fn new(effect: bool) -> Self {
        let mut runtime = UiRuntime::new(1);
        let mut player = PlayerRuntime(player_state::PlayerState::new(1));
        runtime
            .publish_local_runtime_id(&mut player, 1, 42)
            .unwrap();
        if effect {
            runtime
                .apply_local_effect(
                    runtime.session_id(),
                    1,
                    ActorEffectEvent {
                        dimension: 0,
                        actor_runtime_id: 42,
                        action: ActorEffectAction::Add,
                        effect_id: 1,
                        amplifier: 0,
                        particles: true,
                        ambient: false,
                        duration_ticks: -1,
                        tick: 100,
                    },
                    0,
                )
                .unwrap();
        }
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<Time<Real>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<ScreenInputConsumed>()
            .init_resource::<client_shell::ui_resources::FrameInputOwner>()
            .insert_resource(UiRuntimeResource(runtime))
            .insert_resource(player)
            .add_systems(
                Update,
                (
                    crate::ui_runtime::input_owner::decide_input_owner,
                    crate::ui_runtime::emotes::drive_emote_input,
                    drive_mob_effect_screen_input,
                    crate::ui_runtime::interaction::drive_chat_keyboard_input,
                )
                    .chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..Default::default()
                },
                CursorOptions {
                    grab_mode: CursorGrabMode::Locked,
                    visible: false,
                    ..Default::default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        Self { app, window }
    }

    fn queue(&mut self, key_code: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key_code);
        self.app.world_mut().write_message(KeyboardInput {
            key_code,
            state: ButtonState::Pressed,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            text: None,
            repeat: false,
            window: self.window,
        });
    }

    fn press(&mut self, key: KeyCode) {
        self.queue(key);
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    fn runtime(&self) -> &UiRuntime {
        self.app.world().resource::<UiRuntimeResource>()
    }

    fn cursor_captured(&self) -> bool {
        let cursor = self.app.world().get::<CursorOptions>(self.window).unwrap();
        cursor.grab_mode != CursorGrabMode::None && !cursor.visible
    }
}

#[test]
fn default_z_opens_and_closes_the_screen_and_owns_its_presses() {
    let mut h = Harness::new(true);
    h.press(KeyCode::KeyZ);
    assert!(h.runtime().mob_effect_screen().is_open());
    assert!(!h.cursor_captured(), "the open screen frees the pointer");
    assert!(h.app.world().resource::<ScreenInputConsumed>().0);
    h.press(KeyCode::KeyZ);
    assert!(!h.runtime().mob_effect_screen().is_open());
    assert!(
        h.cursor_captured(),
        "closing returns the pointer to gameplay"
    );
}

#[test]
fn z_without_an_active_effect_leaves_gameplay_input_alone() {
    let mut h = Harness::new(false);
    h.queue(KeyCode::KeyZ);
    h.app.update();
    assert!(!h.runtime().mob_effect_screen().is_open());
    assert!(!h.app.world().resource::<ScreenInputConsumed>().0);
    assert!(
        h.app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyZ)
    );
}

#[test]
fn escape_closes_the_screen_without_passing_its_frame_to_chat() {
    let mut h = Harness::new(true);
    h.press(KeyCode::KeyZ);
    assert!(h.runtime().mob_effect_screen().is_open());
    h.queue(KeyCode::Escape);
    h.queue(KeyCode::KeyT);
    h.app.update();
    assert!(!h.runtime().mob_effect_screen().is_open());
    assert!(!h.runtime().chat_focused(), "the closing frame opened chat");
    assert!(
        !h.app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape),
        "Escape must not also open the pause menu"
    );
}

#[test]
fn credits_keep_their_input_while_an_effect_is_active() {
    let mut h = Harness::new(true);
    h.app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .credits_mut()
        .open(42, 1, 0);
    h.queue(KeyCode::KeyZ);
    h.app.update();
    assert!(!h.runtime().mob_effect_screen().is_open());
    assert!(
        !h.app.world().resource::<ScreenInputConsumed>().0,
        "the credits screen must still see the frame's keys"
    );
}

#[test]
fn a_mouse_remap_opens_and_closes_the_screen_instead_of_z() {
    let mut h = Harness::new(true);
    let mut menu = MenuRuntime::new(false, 2, "Bindings".into());
    let row = BindingId::MobEffects.row(false).unwrap();
    assert!(client_shell::menu::test_support::remap_control(
        &mut menu,
        row,
        semantic_input::PhysicalControl::MouseButton(
            client_shell::physical_input::mouse_button_code(MouseButton::Back).unwrap()
        )
    ));
    h.app.insert_resource(menu);
    h.press(KeyCode::KeyZ);
    assert!(
        !h.runtime().mob_effect_screen().is_open(),
        "Z is no longer bound"
    );
    for open in [true, false] {
        h.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Back);
        h.app.update();
        assert_eq!(h.runtime().mob_effect_screen().is_open(), open);
        assert!(
            !h.app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .just_pressed(MouseButton::Back)
        );
        h.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        h.app.update();
    }
}

/// A sign editor's closing frame, standing in for any screen's: its keys are not the game's.
#[test]
fn the_frame_a_screen_closes_does_not_open_the_screen() {
    use crate::ui_runtime::{emotes::drive_emote_input, input_owner::decide_input_owner};
    fn close_sign(mut runtime: ResMut<UiRuntimeResource>) {
        runtime.sign_editor_mut().close();
    }
    let mut h = Harness::new(true);
    h.app.add_systems(
        Update,
        close_sign
            .after(decide_input_owner)
            .before(drive_emote_input),
    );
    h.app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .sign_editor_mut()
        .open(client_ui::ui_runtime::sign_editor::SignEdit::new(
            [0, 64, 0],
            true,
            Default::default(),
        ));
    h.press(KeyCode::KeyZ);
    assert!(!h.runtime().mob_effect_screen().is_open());
    h.press(KeyCode::KeyZ);
    assert!(h.runtime().mob_effect_screen().is_open());
}

#[test]
fn a_frame_the_emote_wheel_consumed_does_not_open_the_screen() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut h = Harness::new(true);
    h.press(KeyCode::KeyB);
    assert!(h.runtime().emotes().is_open());
    h.queue(KeyCode::Escape);
    h.queue(KeyCode::KeyZ);
    h.app.update();
    assert!(!h.runtime().emotes().is_open());
    assert!(
        !h.runtime().mob_effect_screen().is_open(),
        "Z arrived in the batch the wheel consumed"
    );
}

#[test]
fn the_emote_key_does_not_open_the_wheel_under_the_screen() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let mut h = Harness::new(true);
    h.press(KeyCode::KeyZ);
    assert!(h.runtime().mob_effect_screen().is_open());
    h.queue(KeyCode::KeyB);
    h.queue(KeyCode::Escape);
    h.app.update();
    assert!(
        !h.runtime().emotes().is_open(),
        "B opened a wheel under the screen"
    );
    assert!(
        !h.runtime().mob_effect_screen().is_open(),
        "Escape belongs to the screen on top"
    );
}

#[test]
fn a_batch_rejected_by_focus_return_does_not_open_the_screen() {
    let mut h = Harness::new(true);
    let mut focus = client_state::CursorFocus::default();
    // Lost and regained in one event batch: the window reports focus, the batch is rejected.
    focus.0.focus_changed(false);
    focus.0.focus_changed(true);
    assert!(!focus.0.available());
    h.app.insert_resource(focus);
    h.queue(KeyCode::KeyZ);
    h.app.update();
    assert!(!h.runtime().mob_effect_screen().is_open());
}

/// View opens the list only with an active effect and owns both opening and closing frames.
#[test]
fn controller_view_opens_and_closes_effects_without_reaching_other_screens() {
    for effect in [false, true] {
        let mut h = Harness::new(effect);
        let pad = h.app.world_mut().spawn(Gamepad::default()).id();
        for expected in [effect, false] {
            h.app
                .world_mut()
                .get_mut::<Gamepad>(pad)
                .unwrap()
                .digital_mut()
                .press(GamepadButton::Select);
            h.app.update();
            assert_eq!(h.runtime().mob_effect_screen().is_open(), expected);
            assert_eq!(h.app.world().resource::<ScreenInputConsumed>().0, effect);
            h.app
                .world_mut()
                .get_mut::<Gamepad>(pad)
                .unwrap()
                .digital_mut()
                .reset_all();
            h.app.update();
        }
    }
}

#[test]
fn controller_view_respects_screen_ownership_and_actionable_toasts() {
    let mut h = Harness::new(true);
    let mut menu = MenuRuntime::new(false, 2, "Effects".into());
    menu.push_join_request(7, "Guest".into(), std::time::Duration::ZERO);
    h.app.insert_resource(menu);
    h.app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .stand_toast(ui::StandingToast {
            id: 7,
            title: "Join request".into(),
            message: "Respond".into(),
            press: Some(ui::ToastPress::JoinRequests),
            since_millis: 0,
            until_millis: 60_000,
        });
    let pad = h.app.world_mut().spawn(Gamepad::default()).id();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::Select);
    h.app.update();
    assert!(!h.runtime().mob_effect_screen().is_open());
    assert!(h.app.world().resource::<MenuRuntime>().is_visible());
    assert!(h.app.world().resource::<ScreenInputConsumed>().0);
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .reset_all();
    h.app.update();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::Select);
    h.app.update();
    assert!(
        !h.runtime().mob_effect_screen().is_open(),
        "pause owns View now"
    );
}

/// A platform touch on the painted icon reaches the same adapter as keyboard and controller.
#[test]
fn touching_the_effect_art_opens_the_list_and_blocks_gameplay() {
    use bevy::input::touch::{TouchInput, TouchPhase, Touches, touch_screen_input_system};
    let Some(mut presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!("skipping effects touch input: missing JSON-UI carrier (make assets)");
        return;
    };
    presentation.enable_touch_controls(true);
    let mut h = Harness::new(true);
    let player = h.app.world().resource::<PlayerRuntime>();
    let before = presentation
        .build(
            player,
            h.runtime(),
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    client_ui::test_support::write_snapshot(&before, "effects_touch_before");
    let point = (0..720)
        .step_by(4)
        .find_map(|y| {
            (0..1280)
                .step_by(4)
                .map(|x| UiPoint::new(x as f32, y as f32).unwrap())
                .find(|point| presentation.mob_effect_hud_hit(*point, [1280.0, 720.0]))
        })
        .expect("the active effect paints a touch target");
    h.app
        .insert_resource(UiPresentationResource(presentation))
        .add_message::<TouchInput>()
        .init_resource::<Touches>()
        .add_systems(PreUpdate, touch_screen_input_system);
    h.app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(point.x(), point.y()),
        window: h.window,
        force: None,
        id: 1,
    });
    h.app.update();
    assert!(h.runtime().mob_effect_screen().is_open());
    assert!(h.app.world().resource::<ScreenInputConsumed>().0);
    assert!(!h.cursor_captured());
    h.app.update();
    assert!(
        h.runtime().mob_effect_screen().is_open(),
        "a held finger must not toggle again"
    );
    h.app
        .world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
            let after = presentation
                .build(
                    world.resource::<PlayerRuntime>(),
                    world.resource::<UiRuntimeResource>(),
                    0,
                    [1280, 720],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            client_ui::test_support::write_snapshot(&after, "effects_touch_after");
        });
}

/// A toast whose press has nothing left to open leaves View to the effects screen.
#[test]
fn controller_view_opens_effects_when_the_toast_press_opens_nothing() {
    let mut h = Harness::new(true);
    h.app
        .insert_resource(MenuRuntime::new(false, 2, "Effects".into()));
    h.app
        .world_mut()
        .resource_mut::<UiRuntimeResource>()
        .stand_toast(ui::StandingToast {
            id: 7,
            title: "Join request".into(),
            message: "Respond".into(),
            press: Some(ui::ToastPress::JoinRequests),
            since_millis: 0,
            until_millis: 60_000,
        });
    let pad = h.app.world_mut().spawn(Gamepad::default()).id();
    h.app
        .world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::Select);
    h.app.update();
    assert!(!h.app.world().resource::<MenuRuntime>().is_visible());
    assert!(h.runtime().mob_effect_screen().is_open());
}
