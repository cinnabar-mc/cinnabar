//! Android controller key codes and joystick axes as Bevy gamepad buttons and axes.

use bevy::prelude::{GamepadAxis, GamepadButton};

use super::gamepads::{GamepadInput as Target, NativeGamepadEvent, changed_inputs};

/// `AINPUT_SOURCE_KEYBOARD`'s device bit, above the button class.
const SOURCE_KEYBOARD_BIT: u32 = 0x0100;
/// `AINPUT_SOURCE_DPAD`'s device bit.
const SOURCE_DPAD_BIT: u32 = 0x0200;
/// `AINPUT_SOURCE_GAMEPAD`'s device bit.
const SOURCE_GAMEPAD_BIT: u32 = 0x0400;
/// `AINPUT_SOURCE_CLASS_JOYSTICK`: sticks, triggers and hats.
pub(super) const SOURCE_CLASS_JOYSTICK: u32 = 0x0010;
/// `AINPUT_SOURCE_GAMEPAD` and `AINPUT_SOURCE_JOYSTICK`, as `InputDevice.getSources()` reports.
pub(super) const SOURCE_GAMEPAD: u32 = 0x0000_0401;
pub(super) const SOURCE_JOYSTICK: u32 = 0x0100_0010;
/// Hat positions past this count as a held D-pad direction.
const HAT_PRESS: f32 = 0.5;

/// The controller button an Android key event reports, if it comes from a controller.
///
/// D-pad key codes are also a keyboard's arrow keys, so they count only from controller
/// sources; every `KEYCODE_BUTTON_*` code is a controller button.
pub(super) fn controller_button(keycode: u32, source: u32) -> Option<GamepadButton> {
    Some(match keycode {
        96 => GamepadButton::South,
        97 => GamepadButton::East,
        98 => GamepadButton::C,
        99 => GamepadButton::West,
        100 => GamepadButton::North,
        101 => GamepadButton::Z,
        102 => GamepadButton::LeftTrigger,
        103 => GamepadButton::RightTrigger,
        104 => GamepadButton::LeftTrigger2,
        105 => GamepadButton::RightTrigger2,
        106 => GamepadButton::LeftThumb,
        107 => GamepadButton::RightThumb,
        108 => GamepadButton::Start,
        109 => GamepadButton::Select,
        110 => GamepadButton::Mode,
        19..=22 if controller_dpad(source) => match keycode {
            19 => GamepadButton::DPadUp,
            20 => GamepadButton::DPadDown,
            21 => GamepadButton::DPadLeft,
            _ => GamepadButton::DPadRight,
        },
        _ => return None,
    })
}

fn controller_dpad(source: u32) -> bool {
    source & SOURCE_GAMEPAD_BIT != 0
        || (source & SOURCE_DPAD_BIT != 0 && source & SOURCE_KEYBOARD_BIT == 0)
}

/// One joystick motion sample's controller axes, in Android's units and directions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct JoystickSample {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub rz: f32,
    pub ltrigger: f32,
    pub rtrigger: f32,
    pub brake: f32,
    pub gas: f32,
    pub hat_x: f32,
    pub hat_y: f32,
}

const TARGETS: [Target; 10] = [
    Target::Axis(GamepadAxis::LeftStickX),
    Target::Axis(GamepadAxis::LeftStickY),
    Target::Axis(GamepadAxis::RightStickX),
    Target::Axis(GamepadAxis::RightStickY),
    Target::Button(GamepadButton::LeftTrigger2),
    Target::Button(GamepadButton::RightTrigger2),
    Target::Button(GamepadButton::DPadLeft),
    Target::Button(GamepadButton::DPadRight),
    Target::Button(GamepadButton::DPadUp),
    Target::Button(GamepadButton::DPadDown),
];

/// Values in `TARGETS` order. Android's stick Y axes point down while Bevy's point up; the
/// right stick is `AXIS_Z`/`AXIS_RZ`, and either trigger axis pair drives the analog triggers.
fn target_values(sample: JoystickSample) -> [f32; 10] {
    let held = |pressed: bool| if pressed { 1.0 } else { 0.0 };
    [
        sample.x,
        -sample.y,
        sample.z,
        -sample.rz,
        sample.ltrigger.max(sample.brake),
        sample.rtrigger.max(sample.gas),
        held(sample.hat_x < -HAT_PRESS),
        held(sample.hat_x > HAT_PRESS),
        held(sample.hat_y < -HAT_PRESS),
        held(sample.hat_y > HAT_PRESS),
    ]
}

/// Last values reported for one controller, so only changes become events.
#[derive(Clone, Debug, Default)]
pub(super) struct ControllerState {
    joystick: Option<[f32; 10]>,
    /// Set once the controller reports trigger axes; its L2/R2 key events then repeat them.
    analog_triggers: bool,
}

impl ControllerState {
    /// Replays a motion event's history in arrival order before its current sample.
    pub(super) fn motion(
        &mut self,
        id: u64,
        history: impl IntoIterator<Item = JoystickSample>,
        current: JoystickSample,
    ) -> Vec<NativeGamepadEvent> {
        history
            .into_iter()
            .chain([current])
            .flat_map(|sample| self.joystick(id, sample))
            .collect()
    }

    /// Reports axis changes without initial zeroes that could release key-only buttons.
    fn joystick(&mut self, id: u64, sample: JoystickSample) -> Vec<NativeGamepadEvent> {
        let values = target_values(sample);
        self.analog_triggers |= values[4] != 0.0 || values[5] != 0.0;
        self.joystick.get_or_insert([0.0; TARGETS.len()]);
        changed_inputs(id, &TARGETS, &mut self.joystick, values)
    }

    /// The event for a controller key edge, or none when trigger axes already report it.
    pub(super) fn key(
        &self,
        id: u64,
        button: GamepadButton,
        pressed: bool,
    ) -> Option<NativeGamepadEvent> {
        let trigger = matches!(
            button,
            GamepadButton::LeftTrigger2 | GamepadButton::RightTrigger2
        );
        (!(trigger && self.analog_triggers)).then_some(NativeGamepadEvent::Button {
            id,
            button,
            value: if pressed { 1.0 } else { 0.0 },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYBOARD: u32 = 0x0101;
    const DPAD: u32 = 0x0201;

    #[test]
    fn face_buttons_map_from_any_source_and_arrows_only_from_controllers() {
        assert_eq!(controller_button(96, KEYBOARD), Some(GamepadButton::South));
        assert_eq!(
            controller_button(103, SOURCE_GAMEPAD),
            Some(GamepadButton::RightTrigger)
        );
        assert_eq!(controller_button(19, KEYBOARD), None);
        assert_eq!(controller_button(19, DPAD), Some(GamepadButton::DPadUp));
        assert_eq!(
            controller_button(22, SOURCE_GAMEPAD | KEYBOARD),
            Some(GamepadButton::DPadRight)
        );
        assert_eq!(controller_button(29, SOURCE_GAMEPAD), None);
    }

    #[test]
    fn first_sample_reports_nonzero_values_then_only_changes() {
        let mut state = ControllerState::default();
        let first = state.joystick(
            1,
            JoystickSample {
                y: -1.0,
                hat_x: 1.0,
                ..JoystickSample::default()
            },
        );
        assert_eq!(first.len(), 2);
        assert!(first.contains(&NativeGamepadEvent::Axis {
            id: 1,
            axis: GamepadAxis::LeftStickY,
            value: 1.0,
        }));
        assert!(first.contains(&NativeGamepadEvent::Button {
            id: 1,
            button: GamepadButton::DPadRight,
            value: 1.0,
        }));
        let next = state.joystick(
            1,
            JoystickSample {
                y: -1.0,
                rz: 0.5,
                ..JoystickSample::default()
            },
        );
        assert_eq!(
            next,
            [
                NativeGamepadEvent::Axis {
                    id: 1,
                    axis: GamepadAxis::RightStickY,
                    value: -0.5,
                },
                NativeGamepadEvent::Button {
                    id: 1,
                    button: GamepadButton::DPadRight,
                    value: 0.0,
                },
            ]
        );
    }

    #[test]
    fn neutral_joystick_axes_preserve_key_only_buttons_until_key_up() {
        for target in TARGETS {
            let Target::Button(button) = target else {
                continue;
            };
            let mut state = ControllerState::default();
            assert_eq!(
                state.key(1, button, true),
                Some(NativeGamepadEvent::Button {
                    id: 1,
                    button,
                    value: 1.0,
                })
            );
            for sample in [
                JoystickSample::default(),
                JoystickSample {
                    x: 0.75,
                    ..JoystickSample::default()
                },
            ] {
                assert!(!state.motion(1, [], sample).iter().any(|event| matches!(
                    event,
                    NativeGamepadEvent::Button { button: changed, .. } if *changed == button
                )));
            }
            assert_eq!(
                state.key(1, button, false),
                Some(NativeGamepadEvent::Button {
                    id: 1,
                    button,
                    value: 0.0,
                })
            );
        }
    }

    #[test]
    fn batched_hat_taps_are_replayed_before_the_current_neutral_sample() {
        let mut state = ControllerState::default();
        state.motion(1, [], JoystickSample::default());
        let events = state.motion(
            1,
            [
                JoystickSample {
                    hat_x: -1.0,
                    ..JoystickSample::default()
                },
                JoystickSample {
                    hat_x: 1.0,
                    ..JoystickSample::default()
                },
            ],
            JoystickSample::default(),
        );
        assert_eq!(
            events,
            [
                NativeGamepadEvent::Button {
                    id: 1,
                    button: GamepadButton::DPadLeft,
                    value: 1.0,
                },
                NativeGamepadEvent::Button {
                    id: 1,
                    button: GamepadButton::DPadLeft,
                    value: 0.0,
                },
                NativeGamepadEvent::Button {
                    id: 1,
                    button: GamepadButton::DPadRight,
                    value: 1.0,
                },
                NativeGamepadEvent::Button {
                    id: 1,
                    button: GamepadButton::DPadRight,
                    value: 0.0,
                },
            ]
        );
        assert!(state.motion(1, [], JoystickSample::default()).is_empty());
    }

    #[test]
    fn trigger_keys_defer_to_trigger_axes_once_reported() {
        let mut state = ControllerState::default();
        assert!(state.key(1, GamepadButton::LeftTrigger2, true).is_some());
        state.joystick(
            1,
            JoystickSample {
                gas: 0.4,
                ..JoystickSample::default()
            },
        );
        assert_eq!(state.key(1, GamepadButton::LeftTrigger2, false), None);
        assert!(state.key(1, GamepadButton::South, false).is_some());
    }
}
