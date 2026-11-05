//! iOS keyboards, mice and controllers through the Game Controller framework, which reports
//! physical keys, raw mouse motion and controllers that winit and gilrs do not, and pointer
//! locking through winit while gameplay owns the pointer.

use std::{
    cell::RefCell,
    ptr::NonNull,
    sync::{Mutex, PoisonError},
};

use bevy::{
    ecs::system::NonSendMarker,
    input::{
        ButtonState,
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
        touch::TouchPhase,
    },
    prelude::*,
    window::PrimaryWindow,
    winit::WINIT_WINDOWS,
};
use block2::RcBlock;
use objc2::{rc::Retained, runtime::Bool};
use objc2_game_controller::{
    GCController, GCControllerButtonInput, GCControllerDirectionPad, GCDevice, GCKeyCode,
    GCKeyboard, GCKeyboardInput, GCMouse, GCMouseInput,
};
use winit::platform::ios::WindowExtIOS;

use super::{
    gamepads::{
        self, GamepadInput, NativeGamepadEvent, NativeGamepadInput, PendingNativeGamepadEvents,
        changed_inputs,
    },
    hid_keys,
};

/// Installs device discovery, event forwarding and pointer locking.
pub(super) fn configure(app: &mut App) {
    gamepads::configure(app);
    app.add_systems(
        PreUpdate,
        (attach_devices, forward_keyboard_mouse)
            .chain()
            .before(NativeGamepadInput)
            .before(bevy::input::InputSystems),
    )
    .add_systems(Last, sync_pointer_lock);
}

/// Keyboard and mouse callbacks queued until the next frame's input update.
#[derive(Default)]
struct Queued {
    keys: Vec<(GCKeyCode, bool)>,
    motion: Vec2,
    buttons: Vec<(MouseButton, bool)>,
    scroll: Vec<Vec2>,
}

static QUEUED: Mutex<Option<Queued>> = Mutex::new(None);

fn queued(apply: impl FnOnce(&mut Queued)) {
    apply(
        QUEUED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(Queued::default),
    );
}

/// Framework devices with installed callbacks, and polled controllers.
#[derive(Default)]
struct Devices {
    keyboard: Option<Retained<GCKeyboardInput>>,
    mice: Vec<Retained<GCMouseInput>>,
    controllers: Vec<Controller>,
    next_id: u64,
}

struct Controller {
    controller: Retained<GCController>,
    id: u64,
    values: Option<[f32; CONTROLLER_INPUTS.len()]>,
}

thread_local! {
    /// Framework objects stay on the main thread, where they were discovered.
    static DEVICES: RefCell<Devices> = RefCell::new(Devices::default());
}

/// Extended gamepad controls in `controller_values` order.
const CONTROLLER_INPUTS: [GamepadInput; 20] = [
    GamepadInput::Axis(GamepadAxis::LeftStickX),
    GamepadInput::Axis(GamepadAxis::LeftStickY),
    GamepadInput::Axis(GamepadAxis::RightStickX),
    GamepadInput::Axis(GamepadAxis::RightStickY),
    GamepadInput::Button(GamepadButton::LeftTrigger2),
    GamepadInput::Button(GamepadButton::RightTrigger2),
    GamepadInput::Button(GamepadButton::South),
    GamepadInput::Button(GamepadButton::East),
    GamepadInput::Button(GamepadButton::West),
    GamepadInput::Button(GamepadButton::North),
    GamepadInput::Button(GamepadButton::LeftTrigger),
    GamepadInput::Button(GamepadButton::RightTrigger),
    GamepadInput::Button(GamepadButton::DPadUp),
    GamepadInput::Button(GamepadButton::DPadDown),
    GamepadInput::Button(GamepadButton::DPadLeft),
    GamepadInput::Button(GamepadButton::DPadRight),
    GamepadInput::Button(GamepadButton::Start),
    GamepadInput::Button(GamepadButton::Select),
    GamepadInput::Button(GamepadButton::LeftThumb),
    GamepadInput::Button(GamepadButton::RightThumb),
];

/// Installs callbacks on newly connected keyboards and mice, and polls controllers.
fn attach_devices(mut pending: ResMut<PendingNativeGamepadEvents>, _main_thread: NonSendMarker) {
    DEVICES.with_borrow_mut(|devices| {
        attach_keyboard(devices);
        attach_mice(devices);
        poll_controllers(devices, &mut pending.0);
    });
}

fn attach_keyboard(devices: &mut Devices) {
    // SAFETY: Main-thread framework getters return retained objects or none.
    let input =
        unsafe { GCKeyboard::coalescedKeyboard().and_then(|keyboard| keyboard.keyboardInput()) };
    if input == devices.keyboard {
        return;
    }
    if let Some(input) = &input {
        let handler = RcBlock::new(
            |_: NonNull<GCKeyboardInput>,
             _: NonNull<GCControllerButtonInput>,
             key: GCKeyCode,
             pressed: Bool| {
                queued(|queued| queued.keys.push((key, pressed.as_bool())));
            },
        );
        // SAFETY: The framework copies the block, whose captures are empty.
        unsafe { input.setKeyChangedHandler(RcBlock::as_ptr(&handler)) };
    }
    devices.keyboard = input;
}

fn attach_mice(devices: &mut Devices) {
    // SAFETY: Main-thread framework getters return retained objects.
    let connected = unsafe { GCMouse::mice() };
    let inputs: Vec<_> = (0..connected.count())
        // SAFETY: As above.
        .filter_map(|index| unsafe { connected.objectAtIndex(index).mouseInput() })
        .collect();
    for input in &inputs {
        if devices.mice.contains(input) {
            continue;
        }
        let moved = RcBlock::new(|_: NonNull<GCMouseInput>, x: f32, y: f32| {
            // Game Controller deltas point up; Bevy's point down.
            queued(|queued| queued.motion += Vec2::new(x, -y));
        });
        let scroll = RcBlock::new(|_: NonNull<GCControllerDirectionPad>, x: f32, y: f32| {
            queued(|queued| queued.scroll.push(Vec2::new(x, y)));
        });
        // SAFETY: The framework copies each block, whose captures are plain values; the
        // buttons are retained by their mouse input.
        unsafe {
            input.setMouseMovedHandler(RcBlock::as_ptr(&moved));
            input
                .scroll()
                .setValueChangedHandler(RcBlock::as_ptr(&scroll));
            for (button, mouse_button) in [
                (Some(input.leftButton()), MouseButton::Left),
                (input.rightButton(), MouseButton::Right),
                (input.middleButton(), MouseButton::Middle),
            ] {
                let Some(button) = button else { continue };
                let handler = RcBlock::new(
                    move |_: NonNull<GCControllerButtonInput>, _: f32, pressed: Bool| {
                        queued(|queued| queued.buttons.push((mouse_button, pressed.as_bool())));
                    },
                );
                button.setPressedChangedHandler(RcBlock::as_ptr(&handler));
            }
        }
    }
    devices.mice = inputs;
}

/// Reports controller connections, removals and changed extended-gamepad values.
fn poll_controllers(devices: &mut Devices, pending: &mut Vec<NativeGamepadEvent>) {
    // SAFETY: Main-thread framework discovery returns retained controllers.
    let connected = unsafe { GCController::controllers() };
    let connected: Vec<_> = (0..connected.count())
        .map(|index| connected.objectAtIndex(index))
        .collect();
    devices.controllers.retain(|known| {
        let present = connected.contains(&known.controller);
        if !present {
            pending.push(NativeGamepadEvent::Disconnected { id: known.id });
        }
        present
    });
    for controller in connected {
        if devices
            .controllers
            .iter()
            .any(|known| known.controller == controller)
        {
            continue;
        }
        let id = devices.next_id;
        devices.next_id += 1;
        // SAFETY: Main-thread property read.
        let name = unsafe { controller.vendorName() }
            .map_or_else(|| "Controller".to_owned(), |name| name.to_string());
        pending.push(NativeGamepadEvent::Connected {
            id,
            name,
            vendor_id: None,
            product_id: None,
        });
        devices.controllers.push(Controller {
            controller,
            id,
            values: None,
        });
    }
    for known in &mut devices.controllers {
        if let Some(values) = controller_values(&known.controller) {
            pending.extend(changed_inputs(
                known.id,
                &CONTROLLER_INPUTS,
                &mut known.values,
                values,
            ));
        }
    }
}

/// Current extended-gamepad values, or none for controllers without that profile.
fn controller_values(controller: &GCController) -> Option<[f32; CONTROLLER_INPUTS.len()]> {
    // SAFETY: Main-thread reads of the controller's current profile state.
    unsafe {
        let pad = controller.extendedGamepad()?;
        let held = |button: Option<Retained<GCControllerButtonInput>>| {
            f32::from(u8::from(button.is_some_and(|button| button.isPressed())))
        };
        let (left, right, dpad) = (pad.leftThumbstick(), pad.rightThumbstick(), pad.dpad());
        Some([
            left.xAxis().value(),
            left.yAxis().value(),
            right.xAxis().value(),
            right.yAxis().value(),
            pad.leftTrigger().value(),
            pad.rightTrigger().value(),
            held(Some(pad.buttonA())),
            held(Some(pad.buttonB())),
            held(Some(pad.buttonX())),
            held(Some(pad.buttonY())),
            held(Some(pad.leftShoulder())),
            held(Some(pad.rightShoulder())),
            held(Some(dpad.up())),
            held(Some(dpad.down())),
            held(Some(dpad.left())),
            held(Some(dpad.right())),
            held(Some(pad.buttonMenu())),
            held(pad.buttonOptions()),
            held(pad.leftThumbstickButton()),
            held(pad.rightThumbstickButton()),
        ])
    }
}

/// Writes queued keyboard and mouse callbacks as Bevy input messages for the primary window.
///
/// While text input is enabled, winit's text path already reports Backspace with the same
/// physical key, so the keyboard's Backspace is dropped there.
fn forward_keyboard_mouse(
    window: Option<Single<(Entity, &Window), With<PrimaryWindow>>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut motion: MessageWriter<MouseMotion>,
    mut wheel: MessageWriter<MouseWheel>,
) {
    let Some(queued) = QUEUED.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return;
    };
    let Some(window) = window else { return };
    let (entity, window) = *window;
    let state = |pressed: bool| {
        if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        }
    };
    for (usage, pressed) in queued.keys {
        let Some(key_code) = u16::try_from(usage).ok().and_then(hid_keys::key_code) else {
            continue;
        };
        if window.ime_enabled && key_code == KeyCode::Backspace {
            continue;
        }
        keys.write(KeyboardInput {
            key_code,
            logical_key: hid_keys::logical_key(key_code),
            state: state(pressed),
            text: None,
            repeat: false,
            window: entity,
        });
    }
    if queued.motion != Vec2::ZERO {
        motion.write(MouseMotion {
            delta: queued.motion,
        });
    }
    for (button, pressed) in queued.buttons {
        buttons.write(MouseButtonInput {
            button,
            state: state(pressed),
            window: entity,
        });
    }
    for scroll in queued.scroll {
        wheel.write(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: scroll.x,
            y: scroll.y,
            window: entity,
            phase: TouchPhase::Moved,
        });
    }
}

/// Locks the iPadOS pointer while gameplay owns it, so mice report only `GCMouse` motion and
/// their clicks do not also arrive as touches.
fn sync_pointer_lock(
    focus: Option<Res<client_state::CursorFocus>>,
    window: Option<Single<Entity, With<PrimaryWindow>>>,
    mut requested: Local<Option<bool>>,
    _main_thread: NonSendMarker,
) {
    let Some(window) = window else { return };
    let wanted = focus.is_some_and(|focus| focus.0.grab_free_capture());
    if *requested == Some(wanted) {
        return;
    }
    WINIT_WINDOWS.with_borrow(|native| {
        if let Some(native) = native.get_window(*window) {
            native.set_prefers_pointer_locked(wanted);
            *requested = Some(wanted);
        }
    });
}
