//! Input control and controller quarantine during transition barriers and active hold tracking.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QuarantinedControl {
    KeyboardUsage(u16),
    MouseButton(u8),
    ControllerButton { device_id: u32, button: u8 },
    ControllerAxis { device_id: u32, axis: u8 },
    TouchContact(u64),
}

pub(super) fn quarantine_active_controls(
    frame: &DeviceFrame,
    output: &mut Vec<QuarantinedControl>,
    settings: &ControlSettings,
) {
    if let Some(keyboard) = frame.keyboard_mouse.as_ref() {
        for code in keyboard.keys.iter().chain(&keyboard.key_edges.pressed) {
            push_quarantine(output, QuarantinedControl::KeyboardUsage(*code));
        }
        for button in keyboard
            .mouse_buttons
            .iter()
            .chain(&keyboard.mouse_edges.pressed)
        {
            push_quarantine(output, QuarantinedControl::MouseButton(*button));
        }
    }
    let controller_state = evaluate_controller_state(frame.controllers.iter(), settings);
    for controller in &frame.controllers {
        for button in controller
            .buttons
            .iter()
            .chain(&controller.button_edges.pressed)
        {
            push_quarantine(
                output,
                QuarantinedControl::ControllerButton {
                    device_id: controller.device_id,
                    button: *button,
                },
            );
        }
        for axis in 0..controller.axes.len() {
            if controller.axes[axis] == 0.0 || !controller_state.axis_family_is_active(axis) {
                continue;
            }
            push_quarantine(
                output,
                QuarantinedControl::ControllerAxis {
                    device_id: controller.device_id,
                    axis: axis as u8,
                },
            );
        }
    }
    for contact in &frame.touches {
        push_quarantine(output, QuarantinedControl::TouchContact(contact.contact_id));
    }
}

pub(super) fn push_quarantine(output: &mut Vec<QuarantinedControl>, control: QuarantinedControl) {
    if !output.contains(&control) {
        if output.len() == MAX_QUARANTINED_CONTROLS {
            output.remove(0);
        }
        output.push(control);
    }
}

pub(super) fn without_quarantined_controls(
    frame: &DeviceFrame,
    quarantine: &[QuarantinedControl],
    quarantined_controllers: &[u32],
    controller_activity_baselines: &[ControllerActivityBaseline],
    settings: &ControlSettings,
) -> DeviceFrame {
    let mut filtered = frame.clone();
    if let Some(keyboard) = filtered.keyboard_mouse.as_mut() {
        for keys in [
            &mut keyboard.keys,
            &mut keyboard.key_edges.pressed,
            &mut keyboard.key_edges.released,
        ] {
            keys.retain(|code| !quarantine.contains(&QuarantinedControl::KeyboardUsage(*code)));
        }
        for buttons in [
            &mut keyboard.mouse_buttons,
            &mut keyboard.mouse_edges.pressed,
            &mut keyboard.mouse_edges.released,
        ] {
            buttons
                .retain(|button| !quarantine.contains(&QuarantinedControl::MouseButton(*button)));
        }
    }
    let controller_state = evaluate_controller_state(filtered.controllers.iter(), settings);
    filtered.controllers.retain_mut(|controller| {
        if quarantined_controllers.contains(&controller.device_id)
            || controller_activity_baselines.iter().any(|baseline| {
                baseline.device_id == controller.device_id
                    && (controller.activity_sequence <= baseline.activity_sequence
                        || controller_state.is_neutral())
            })
        {
            return false;
        }
        for buttons in [
            &mut controller.buttons,
            &mut controller.button_edges.pressed,
            &mut controller.button_edges.released,
        ] {
            buttons.retain(|button| {
                !quarantine.contains(&QuarantinedControl::ControllerButton {
                    device_id: controller.device_id,
                    button: *button,
                })
            });
        }
        for (axis, value) in controller.axes.iter_mut().enumerate() {
            if quarantine.contains(&QuarantinedControl::ControllerAxis {
                device_id: controller.device_id,
                axis: axis as u8,
            }) {
                *value = 0.0;
            }
        }
        true
    });
    // A neutral controller family cannot compete with another present device
    // family. Preserve the sole-controller mode fallback without allowing it
    // to suppress semantically active keyboard or touch input.
    if evaluate_controller_state(filtered.controllers.iter(), settings).is_neutral()
        && (filtered.keyboard_mouse.is_some() || !filtered.touches.is_empty())
    {
        filtered.controllers.clear();
    }
    filtered.touches.retain(|contact| {
        !quarantine.contains(&QuarantinedControl::TouchContact(contact.contact_id))
    });
    filtered
}
