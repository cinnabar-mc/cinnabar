//! macOS fallback for already-streaming Switch full-input reports ignored by gilrs.

mod native;
mod report;

use std::collections::HashMap;

use bevy::{
    input::{
        InputSystems,
        gamepad::{
            GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
            RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
        },
    },
    prelude::*,
};

use report::{PRODUCT_ID, State, VENDOR_ID};

pub(crate) struct SwitchGamepadPlugin;

impl Plugin for SwitchGamepadPlugin {
    fn build(&self, app: &mut App) {
        match native::Listener::start() {
            Ok(listener) => {
                app.insert_resource(Bridge {
                    listener,
                    devices: HashMap::new(),
                    retired: Vec::new(),
                })
                .add_systems(PreUpdate, publish.before(InputSystems))
                .add_systems(PreUpdate, retire.after(InputSystems));
            }
            Err(error) => warn!(%error, "Switch HID listener could not start"),
        }
    }
}

#[derive(Resource)]
struct Bridge {
    listener: native::Listener,
    devices: HashMap<usize, Entity>,
    retired: Vec<Entity>,
}

// Physical positions, not Nintendo legends: B is south and A is east.
const BUTTONS: &[(u32, GamepadButton)] = &[
    (1 << 0, GamepadButton::West),
    (1 << 1, GamepadButton::North),
    (1 << 2, GamepadButton::South),
    (1 << 3, GamepadButton::East),
    (1 << 6, GamepadButton::RightTrigger),
    (1 << 7, GamepadButton::RightTrigger2),
    (1 << 8, GamepadButton::Select),
    (1 << 9, GamepadButton::Start),
    (1 << 10, GamepadButton::RightThumb),
    (1 << 11, GamepadButton::LeftThumb),
    (1 << 12, GamepadButton::Mode),
    (1 << 13, GamepadButton::Other(0)),
    (1 << 16, GamepadButton::DPadDown),
    (1 << 17, GamepadButton::DPadUp),
    (1 << 18, GamepadButton::DPadRight),
    (1 << 19, GamepadButton::DPadLeft),
    (1 << 22, GamepadButton::LeftTrigger),
    (1 << 23, GamepadButton::LeftTrigger2),
];
const AXES: [GamepadAxis; 4] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
];

/// Publish through Bevy's normal filtering, deadzones and pressed/released lifecycle.
fn publish(
    mut commands: Commands,
    mut bridge: ResMut<Bridge>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut events: MessageWriter<RawGamepadEvent>,
) {
    let incoming: Vec<_> = bridge
        .listener
        .receiver
        .lock()
        .expect("Switch HID receiver poisoned")
        .try_iter()
        .collect();
    for event in incoming {
        match event {
            native::Event::State(device, state) => {
                let entity = *bridge.devices.entry(device).or_insert_with(|| {
                    let entity = commands.spawn_empty().id();
                    let event = GamepadConnectionEvent::new(
                        entity,
                        GamepadConnection::Connected {
                            name: "Switch full-report controller".into(),
                            vendor_id: Some(VENDOR_ID),
                            product_id: Some(PRODUCT_ID),
                        },
                    );
                    connections.write(event.clone());
                    events.write(RawGamepadEvent::Connection(event));
                    info!(?entity, "Switch full-report controller input detected");
                    entity
                });
                publish_state(entity, state, &mut events);
            }
            native::Event::Removed(device) => {
                let Some(entity) = bridge.devices.remove(&device) else {
                    continue;
                };
                let event = GamepadConnectionEvent::new(entity, GamepadConnection::Disconnected);
                connections.write(event.clone());
                events.write(RawGamepadEvent::Connection(event));
                bridge.retired.push(entity);
            }
        }
    }
}

/// Keep disconnected entities alive until Bevy has consumed their connection messages.
fn retire(mut commands: Commands, mut bridge: ResMut<Bridge>) {
    for entity in bridge.retired.drain(..) {
        commands.entity(entity).despawn();
    }
}

fn publish_state(entity: Entity, state: State, events: &mut MessageWriter<RawGamepadEvent>) {
    for &(mask, button) in BUTTONS {
        events.write(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
            entity,
            button,
            if state.buttons & mask == 0 { 0.0 } else { 1.0 },
        )));
    }
    for (axis, value) in AXES.into_iter().zip(state.axes) {
        events.write(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
            entity, axis, value,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, std::sync::mpsc::Sender<native::Event>) {
        let (sender, listener) = native::Listener::test_channel();
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(Bridge {
                listener,
                devices: HashMap::new(),
                retired: Vec::new(),
            })
            .add_systems(PreUpdate, publish.before(InputSystems))
            .add_systems(PreUpdate, retire.after(InputSystems));
        (app, sender)
    }

    #[test]
    fn raw_switch_state_reaches_bevy_and_releases_on_following_frame() {
        let (mut app, sender) = fixture();
        let mut payload = [0; report::PAYLOAD_LENGTH];
        payload[5..11].copy_from_slice(&[0, 8, 128, 0, 8, 128]);
        payload[2] = 1 << 2;
        let pressed = State::decode(report::REPORT_ID, &payload).unwrap();
        sender.send(native::Event::State(7, pressed)).unwrap();
        app.update();
        let entity = app.world().resource::<Bridge>().devices[&7];
        let pad = app.world().get::<Gamepad>(entity).unwrap();
        assert!(pad.just_pressed(GamepadButton::South));
        assert!(!pad.pressed(GamepadButton::East));

        sender
            .send(native::Event::State(
                7,
                State {
                    buttons: 0,
                    axes: [1.0, -1.0, 0.0, 0.0],
                },
            ))
            .unwrap();
        app.update();
        let pad = app.world().get::<Gamepad>(entity).unwrap();
        assert!(pad.just_released(GamepadButton::South));
        assert_eq!(pad.get(GamepadAxis::LeftStickX), Some(1.0));
        assert_eq!(pad.get(GamepadAxis::LeftStickY), Some(-1.0));

        sender.send(native::Event::Removed(7)).unwrap();
        app.update();
        assert!(app.world().get::<Gamepad>(entity).is_none());
        assert!(app.world().resource::<Bridge>().devices.is_empty());
    }

    #[test]
    fn identical_pads_have_independent_lifetimes_and_button_states() {
        let (mut app, sender) = fixture();
        for (device, buttons) in [(1, 1 << 2), (2, 1 << 3)] {
            sender
                .send(native::Event::State(
                    device,
                    State {
                        buttons,
                        axes: [0.0; 4],
                    },
                ))
                .unwrap();
        }
        app.update();
        let devices = app.world().resource::<Bridge>().devices.clone();
        assert_ne!(devices[&1], devices[&2]);
        assert!(
            app.world()
                .get::<Gamepad>(devices[&1])
                .unwrap()
                .pressed(GamepadButton::South)
        );
        assert!(
            app.world()
                .get::<Gamepad>(devices[&2])
                .unwrap()
                .pressed(GamepadButton::East)
        );
        sender.send(native::Event::Removed(1)).unwrap();
        app.update();
        assert!(
            app.world()
                .get::<Gamepad>(devices[&2])
                .unwrap()
                .pressed(GamepadButton::East)
        );
    }

    #[test]
    fn physical_button_mapping_is_unique_and_covers_supported_protocol_bits() {
        let mut masks = 0;
        for (index, &(mask, button)) in BUTTONS.iter().enumerate() {
            assert!(mask.is_power_of_two());
            assert_eq!(masks & mask, 0);
            assert!(
                BUTTONS[..index]
                    .iter()
                    .all(|&(_, previous)| previous != button)
            );
            masks |= mask;
        }
        // Protocol byte 3: face/shoulder; byte 4: system; byte 5: D-pad/shoulder.
        assert_eq!(masks, 0xcf | (0x3f << 8) | (0xcf << 16));
    }
}
