//! Native controller events for platforms gilrs has no backend for, forwarded as Bevy's raw
//! gamepad events so every gamepad consumer reads them like a gilrs controller.

use std::{collections::HashMap, sync::Mutex};

use bevy::{
    input::gamepad::{
        GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
        RawGamepadButtonChangedEvent, RawGamepadEvent,
    },
    prelude::*,
};

/// One controller event reported by a platform callback, keyed by the platform's device id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeGamepadEvent {
    Connected {
        id: u64,
        name: String,
        vendor_id: Option<u16>,
        product_id: Option<u16>,
    },
    Disconnected {
        id: u64,
    },
    Button {
        id: u64,
        button: GamepadButton,
        value: f32,
    },
    Axis {
        id: u64,
        axis: GamepadAxis,
        value: f32,
    },
}

impl NativeGamepadEvent {
    const fn id(&self) -> u64 {
        match self {
            Self::Connected { id, .. }
            | Self::Disconnected { id }
            | Self::Button { id, .. }
            | Self::Axis { id, .. } => *id,
        }
    }
}

/// Events queued by platform input callbacks, which run outside the Bevy schedule.
static QUEUED: Mutex<Vec<NativeGamepadEvent>> = Mutex::new(Vec::new());

/// Queues events from a platform callback for the next frame's input update.
pub(crate) fn queue(events: impl IntoIterator<Item = NativeGamepadEvent>) {
    QUEUED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(events);
}

/// Events taken from the platform queue this frame, in arrival order.
#[derive(Resource, Default)]
pub(crate) struct PendingNativeGamepadEvents(pub(crate) Vec<NativeGamepadEvent>);

/// Maps platform device ids to their gamepad entities while they stay connected.
#[derive(Resource, Default)]
pub(crate) struct NativeGamepads {
    entities: HashMap<u64, Entity>,
}

impl NativeGamepads {
    /// Platform device ids currently connected.
    pub(crate) fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.entities.keys().copied()
    }
}

/// Registers forwarding ahead of Bevy's gamepad processing, matching the gilrs backend.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<PendingNativeGamepadEvents>()
        .init_resource::<NativeGamepads>()
        .add_systems(
            PreUpdate,
            (take_queued_events, forward_native_gamepad_events)
                .chain()
                .in_set(NativeGamepadInput)
                .before(bevy::input::InputSystems),
        );
}

/// Platform controller events reach Bevy's raw gamepad messages in this set.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NativeGamepadInput;

fn take_queued_events(mut pending: ResMut<PendingNativeGamepadEvents>) {
    let mut queued = QUEUED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pending.0.append(&mut queued);
}

/// Writes pending events as Bevy gamepad messages. A device that reports input before its
/// connection event is connected first under a generic name, so no input is dropped.
pub(crate) fn forward_native_gamepad_events(
    mut commands: Commands,
    mut pending: ResMut<PendingNativeGamepadEvents>,
    mut gamepads: ResMut<NativeGamepads>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axes: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    for event in pending.0.drain(..) {
        let id = event.id();
        if let NativeGamepadEvent::Disconnected { .. } = event {
            if let Some(entity) = gamepads.entities.remove(&id) {
                let event = GamepadConnectionEvent::new(entity, GamepadConnection::Disconnected);
                raw.write(event.clone().into());
                connections.write(event);
            }
            continue;
        }
        let entity = match gamepads.entities.get(&id) {
            Some(entity) => *entity,
            None => {
                let entity = commands.spawn_empty().id();
                gamepads.entities.insert(id, entity);
                let connection = match &event {
                    NativeGamepadEvent::Connected {
                        name,
                        vendor_id,
                        product_id,
                        ..
                    } => GamepadConnection::Connected {
                        name: name.clone(),
                        vendor_id: *vendor_id,
                        product_id: *product_id,
                    },
                    _ => GamepadConnection::Connected {
                        name: "Controller".to_owned(),
                        vendor_id: None,
                        product_id: None,
                    },
                };
                let event = GamepadConnectionEvent::new(entity, connection);
                raw.write(event.clone().into());
                connections.write(event);
                entity
            }
        };
        match event {
            NativeGamepadEvent::Button { button, value, .. } => {
                let event = RawGamepadButtonChangedEvent::new(entity, button, value);
                raw.write(event.into());
                buttons.write(event);
            }
            NativeGamepadEvent::Axis { axis, value, .. } => {
                let event = RawGamepadAxisChangedEvent::new(entity, axis, value);
                raw.write(event.into());
                axes.write(event);
            }
            NativeGamepadEvent::Connected { .. } | NativeGamepadEvent::Disconnected { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::{InputPlugin, gamepad::Gamepad};

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(InputPlugin)
            .init_resource::<PendingNativeGamepadEvents>()
            .init_resource::<NativeGamepads>()
            .add_systems(
                PreUpdate,
                forward_native_gamepad_events.before(bevy::input::InputSystems),
            );
        app
    }

    fn send(app: &mut App, events: impl IntoIterator<Item = NativeGamepadEvent>) {
        app.world_mut()
            .resource_mut::<PendingNativeGamepadEvents>()
            .0
            .extend(events);
        app.update();
    }

    /// Each connected pad's name, whether South is held and whether the left stick points right.
    fn pads(app: &mut App) -> Vec<(String, bool, bool)> {
        app.world_mut()
            .query::<(&Name, &Gamepad)>()
            .iter(app.world())
            .map(|(name, pad)| {
                (
                    name.to_string(),
                    pad.pressed(GamepadButton::South),
                    pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0) > 0.5,
                )
            })
            .collect()
    }

    #[test]
    fn input_before_connection_connects_the_device_without_dropping_the_input() {
        let mut app = app();
        send(
            &mut app,
            [
                NativeGamepadEvent::Button {
                    id: 7,
                    button: GamepadButton::South,
                    value: 1.0,
                },
                NativeGamepadEvent::Axis {
                    id: 7,
                    axis: GamepadAxis::LeftStickX,
                    value: 0.75,
                },
            ],
        );
        assert_eq!(pads(&mut app), [("Controller".to_owned(), true, true)]);
    }

    #[test]
    fn connection_names_the_device_and_disconnection_removes_it() {
        let mut app = app();
        send(
            &mut app,
            [NativeGamepadEvent::Connected {
                id: 3,
                name: "Pad".to_owned(),
                vendor_id: Some(0x45e),
                product_id: Some(0xb13),
            }],
        );
        assert_eq!(pads(&mut app), [("Pad".to_owned(), false, false)]);
        send(&mut app, [NativeGamepadEvent::Disconnected { id: 3 }]);
        assert!(pads(&mut app).is_empty());
        assert_eq!(app.world().resource::<NativeGamepads>().ids().count(), 0);
    }

    #[test]
    fn unknown_disconnections_are_ignored() {
        let mut app = app();
        send(&mut app, [NativeGamepadEvent::Disconnected { id: 9 }]);
        assert!(pads(&mut app).is_empty());
    }
}
