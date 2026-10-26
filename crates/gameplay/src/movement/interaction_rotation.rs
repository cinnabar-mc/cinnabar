//! Interaction aim stays separate from actor facing and camera presentation.

use super::MovementTicker;

/// The float conversion used when publishing a picked direction.
const DEGREES_PER_RADIAN: f32 = 57.295_776;

/// Converts a finite, nonzero ray into interaction pitch and yaw.
fn from_direction([x, y, z]: [f32; 3]) -> Option<[f32; 2]> {
    let horizontal = (x * x + z * z).sqrt();
    if ![x, y, z, horizontal].into_iter().all(f32::is_finite) || (horizontal == 0.0 && y == 0.0) {
        return None;
    }
    Some([
        -y.atan2(horizontal) * DEGREES_PER_RADIAN,
        z.atan2(x) * DEGREES_PER_RADIAN - 90.0,
    ])
}

/// Samples ordinary crosshair aim through the movement direction lookup table.
pub(super) fn from_facing(pitch: f32, yaw: f32) -> [f32; 2] {
    let direction = sim::view_direction(pitch, yaw);
    from_direction([direction.x as f32, direction.y as f32, direction.z as f32])
        .unwrap_or([pitch, yaw])
}

impl MovementTicker {
    /// Publishes assisted aim for one unsent tick; a policy without a target uses actor facing.
    pub fn publish_interaction_aim(&mut self, tick: u64, direction: Option<[f32; 3]>) -> bool {
        let rotation = match direction.map(from_direction) {
            Some(Some(rotation)) => Some(rotation),
            Some(None) => return false,
            None => None,
        };
        let Some(sample) = self
            .outbox
            .iter_mut()
            .find(|sample| sample.snapshot.tick == tick)
        else {
            return false;
        };
        sample.snapshot.interact_rotation =
            rotation.unwrap_or([sample.snapshot.pitch, sample.snapshot.yaw]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_directions_keep_their_independent_pitch_and_unwrapped_yaw() {
        for (direction, expected) in [
            ([0.0, 0.0, 1.0], [0.0, 0.0]),
            ([1.0, 0.0, 0.0], [0.0, -90.0]),
            ([-1.0, 1.0, 0.0], [-45.0, 90.0]),
            ([0.0, -1.0, -1.0], [45.0, -180.0]),
        ] {
            let actual = from_direction(direction).unwrap();
            assert!(
                actual
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 0.000_1)
            );
        }
        assert!(from_direction([0.0; 3]).is_none());
        assert!(from_direction([f32::NAN, 0.0, 1.0]).is_none());
    }
}
