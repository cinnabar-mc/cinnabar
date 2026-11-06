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
    /// Sets the interaction model the touch control mode reports while input is touch.
    pub fn set_touch_control_mode(&mut self, mode: semantic_input::TouchControlMode) {
        self.touch_interaction_model = match mode {
            semantic_input::TouchControlMode::Touch => protocol::PlayerInteractionModel::Touch,
            semantic_input::TouchControlMode::Crosshair => {
                protocol::PlayerInteractionModel::Crosshair
            }
            semantic_input::TouchControlMode::Classic => protocol::PlayerInteractionModel::Classic,
        };
    }

    /// The interaction model reported for a tick sampled from `input_mode`. Only touch input
    /// has a control mode; other inputs aim with the crosshair.
    pub(super) fn interaction_model(
        &self,
        input_mode: protocol::PlayerInputMode,
    ) -> protocol::PlayerInteractionModel {
        if input_mode == protocol::PlayerInputMode::Touch {
            self.touch_interaction_model
        } else {
            protocol::PlayerInteractionModel::Crosshair
        }
    }

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

    /// Touch ticks report the touch control mode's interaction model; other inputs report the
    /// crosshair, whatever the touch mode.
    #[test]
    fn only_touch_ticks_report_the_touch_interaction_model() {
        use crate::{movement::MovementSource, test_support::survival_mining::completed};
        use protocol::{PlayerInputMode, PlayerInteractionModel};
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, completed(101).position);
        ticker.set_source(MovementSource::Physics);
        ticker.set_touch_control_mode(semantic_input::TouchControlMode::Classic);
        for (tick, input_mode) in [(101, PlayerInputMode::Touch), (102, PlayerInputMode::Mouse)] {
            let mut sample = completed(tick);
            sample.input_mode = input_mode;
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        let models: Vec<_> = ticker
            .outbox
            .iter()
            .map(|sample| sample.snapshot.interaction_model)
            .collect();
        assert_eq!(
            models,
            [
                PlayerInteractionModel::Classic,
                PlayerInteractionModel::Crosshair
            ]
        );
    }

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
