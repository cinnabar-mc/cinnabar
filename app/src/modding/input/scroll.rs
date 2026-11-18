//! Preserve native wheel units before a mod may claim gameplay scrolling.

use bevy::{
    ecs::message::MessageCursor,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
};

/// Reads each primary-window wheel event once, keeping line and pixel deltas independent.
/// The boolean retains event presence even when deltas cancel to zero.
pub(super) fn sample(
    cursor: &mut MessageCursor<MouseWheel>,
    events: Option<&Messages<MouseWheel>>,
    window: Entity,
) -> (mod_host::ScrollFrame, bool) {
    let mut frame = mod_host::empty_scroll();
    let mut received = false;
    if let Some(events) = events {
        for event in cursor.read(events).filter(|event| event.window == window) {
            received = true;
            let (x, y) = match event.unit {
                MouseScrollUnit::Line => (&mut frame.line_x, &mut frame.line_y),
                MouseScrollUnit::Pixel => (&mut frame.pixel_x, &mut frame.pixel_y),
            };
            for (total, delta) in [(x, event.x), (y, event.y)] {
                let sum = *total + delta;
                if sum.is_finite() {
                    *total = sum;
                }
            }
        }
    }
    (frame, received)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_units_axes_and_window_ownership_survive_without_replay() {
        let window = Entity::from_raw_u32(1).unwrap();
        let other = Entity::from_raw_u32(2).unwrap();
        let mut events = Messages::default();
        for (unit, x, y, window) in [
            (MouseScrollUnit::Line, 0.5, 2.0, window),
            (MouseScrollUnit::Pixel, -4.0, -12.0, window),
            (MouseScrollUnit::Line, 0.0, 99.0, other),
            (MouseScrollUnit::Line, f32::NAN, 0.0, window),
        ] {
            events.write(MouseWheel {
                unit,
                x,
                y,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
        }
        let mut cursor = MessageCursor::default();
        let (frame, received) = sample(&mut cursor, Some(&events), window);
        assert!(received);
        assert_eq!(
            [frame.line_x, frame.line_y, frame.pixel_x, frame.pixel_y],
            [0.5, 2.0, -4.0, -12.0]
        );
        assert_eq!(
            sample(&mut cursor, Some(&events), window),
            (mod_host::empty_scroll(), false)
        );
    }

    #[test]
    fn cancelling_deltas_still_identify_input_that_must_be_claimed() {
        let window = Entity::from_raw_u32(1).unwrap();
        let mut events = Messages::default();
        for y in [1.0, -1.0] {
            events.write(MouseWheel {
                unit: MouseScrollUnit::Line,
                x: 0.0,
                y,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
        }
        assert_eq!(
            sample(&mut MessageCursor::default(), Some(&events), window),
            (mod_host::empty_scroll(), true)
        );
    }
}
