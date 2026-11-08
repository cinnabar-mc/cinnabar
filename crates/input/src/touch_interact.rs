//! Touch control modes and the tap, hold and turn gestures of the world under the controls.

use std::time::Duration;

/// Bedrock's touch control mode, which a touch client also reports as its interaction model.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TouchControlMode {
    /// Joystick movement; tapping or holding the world interacts where the finger is.
    #[default]
    Touch,
    /// Joystick movement; attack and use buttons act on the crosshair.
    Crosshair,
    /// Classic D-pad movement; tapping or holding the world interacts where the finger is.
    Classic,
}

impl TouchControlMode {
    /// Whether the world takes taps and holds and picks along the finger instead of the crosshair.
    #[must_use]
    pub const fn interacts_by_touch(self) -> bool {
        !matches!(self, Self::Crosshair)
    }

    /// Whether movement uses the classic D-pad instead of the joystick.
    #[must_use]
    pub const fn uses_dpad(self) -> bool {
        matches!(self, Self::Classic)
    }
}

/// Travel, in screen widths, that turns a pending touch into a camera drag.
const TURN_THRESHOLD: f32 = 0.05;
/// A single movement step longer than this many pixels is a jump in the reported position
/// rather than finger travel.
const MAX_STEP_PIXELS: f32 = 200.0;
/// A still finger held this long becomes a hold.
const HOLD_AFTER: Duration = Duration::from_millis(400);
/// A still finger lifted within this long is a tap.
const TAP_WITHIN: Duration = Duration::from_millis(250);

/// What a world touch is currently doing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    /// Not yet classified; neither looks nor interacts.
    Pending,
    /// Dragging the camera.
    Turning,
    /// Held still past the hold time; interacts until it lifts or starts turning.
    Holding,
}

/// A world touch's classified gesture this frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchGesture {
    /// Lifted quickly without moving.
    Tap,
    /// Became a hold this frame.
    HoldStarted,
    /// A hold ended by lifting or by starting to turn.
    HoldEnded,
}

#[derive(Clone, Copy, Debug)]
struct Gesture {
    contact_id: u64,
    started: Duration,
    travel: f32,
    phase: Phase,
}

/// Classifies fingers on the world as taps, holds or camera drags, as the touch and classic
/// modes do. The caller supplies `now` and each finger's movement in screen widths.
#[derive(Clone, Debug, Default)]
pub struct TouchGestures {
    gestures: Vec<Gesture>,
}

impl TouchGestures {
    /// Starts classifying a finger that pressed the world.
    pub fn begin(&mut self, contact_id: u64, now: Duration) {
        self.gestures
            .retain(|gesture| gesture.contact_id != contact_id);
        self.gestures.push(Gesture {
            contact_id,
            started: now,
            travel: 0.0,
            phase: Phase::Pending,
        });
    }

    /// Advances a held finger by `step`, its movement since the last call in screen widths, with
    /// `width` the screen width in pixels. Returns whether the finger now turns the camera and
    /// any gesture it completed.
    pub fn advance(
        &mut self,
        contact_id: u64,
        step: f32,
        width: f32,
        now: Duration,
    ) -> (bool, Option<TouchGesture>) {
        let Some(gesture) = self.find(contact_id) else {
            return (false, None);
        };
        if step.is_finite() && step * width.max(1.0) <= MAX_STEP_PIXELS {
            gesture.travel += step.abs();
        }
        let moved = gesture.travel >= TURN_THRESHOLD;
        let event = match gesture.phase {
            Phase::Pending if moved => {
                gesture.phase = Phase::Turning;
                None
            }
            Phase::Pending if now.saturating_sub(gesture.started) >= HOLD_AFTER => {
                gesture.phase = Phase::Holding;
                Some(TouchGesture::HoldStarted)
            }
            Phase::Holding if moved => {
                gesture.phase = Phase::Turning;
                Some(TouchGesture::HoldEnded)
            }
            _ => None,
        };
        (gesture.phase == Phase::Turning, event)
    }

    /// Whether a finger is being classified.
    #[must_use]
    pub fn contains(&self, contact_id: u64) -> bool {
        self.gestures
            .iter()
            .any(|gesture| gesture.contact_id == contact_id)
    }

    /// Forgets fingers that `keep` rejects without completing a gesture, as on a cancelled touch.
    pub fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.gestures.retain(|gesture| keep(gesture.contact_id));
    }

    /// Finishes a lifted finger, returning a tap or the end of its hold.
    pub fn end(&mut self, contact_id: u64, now: Duration) -> Option<TouchGesture> {
        let index = self
            .gestures
            .iter()
            .position(|gesture| gesture.contact_id == contact_id)?;
        let gesture = self.gestures.remove(index);
        match gesture.phase {
            Phase::Pending
                if gesture.travel <= TURN_THRESHOLD
                    && now.saturating_sub(gesture.started) <= TAP_WITHIN =>
            {
                Some(TouchGesture::Tap)
            }
            Phase::Holding => Some(TouchGesture::HoldEnded),
            _ => None,
        }
    }

    /// Forgets every finger without completing a gesture, as on focus loss.
    pub fn clear(&mut self) {
        self.gestures.clear();
    }

    fn find(&mut self, contact_id: u64) -> Option<&mut Gesture> {
        self.gestures
            .iter_mut()
            .find(|gesture| gesture.contact_id == contact_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: f32 = 1000.0;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn only_the_classic_mode_uses_the_dpad_and_crosshair_aims_by_crosshair() {
        assert!(TouchControlMode::Classic.uses_dpad() && !TouchControlMode::Touch.uses_dpad());
        assert!(!TouchControlMode::Crosshair.interacts_by_touch());
    }

    #[test]
    fn a_quick_still_lift_taps_and_a_slow_one_does_nothing() {
        let mut gestures = TouchGestures::default();
        gestures.begin(1, ms(0));
        assert_eq!(gestures.advance(1, 0.01, WIDTH, ms(100)), (false, None));
        assert_eq!(gestures.end(1, ms(250)), Some(TouchGesture::Tap));
        gestures.begin(2, ms(0));
        assert_eq!(gestures.end(2, ms(300)), None);
    }

    #[test]
    fn a_still_finger_holds_until_it_lifts_or_turns() {
        let mut gestures = TouchGestures::default();
        gestures.begin(1, ms(0));
        assert_eq!(gestures.advance(1, 0.0, WIDTH, ms(399)), (false, None));
        assert_eq!(
            gestures.advance(1, 0.0, WIDTH, ms(400)),
            (false, Some(TouchGesture::HoldStarted))
        );
        assert_eq!(gestures.end(1, ms(900)), Some(TouchGesture::HoldEnded));
        gestures.begin(2, ms(0));
        gestures.advance(2, 0.0, WIDTH, ms(500));
        assert_eq!(
            gestures.advance(2, 0.06, WIDTH, ms(520)),
            (true, Some(TouchGesture::HoldEnded))
        );
        assert_eq!(gestures.end(2, ms(600)), None);
    }

    #[test]
    fn travel_past_the_threshold_turns_and_never_taps() {
        let mut gestures = TouchGestures::default();
        gestures.begin(1, ms(0));
        assert_eq!(gestures.advance(1, 0.03, WIDTH, ms(16)), (false, None));
        assert_eq!(gestures.advance(1, 0.03, WIDTH, ms(32)), (true, None));
        assert_eq!(gestures.advance(1, 0.0, WIDTH, ms(800)), (true, None));
        assert_eq!(gestures.end(1, ms(810)), None);
    }

    #[test]
    fn a_position_jump_is_not_travel() {
        let mut gestures = TouchGestures::default();
        gestures.begin(1, ms(0));
        // 0.3 screen widths of a 1000-pixel screen is a 300-pixel step.
        assert_eq!(gestures.advance(1, 0.3, WIDTH, ms(16)), (false, None));
        assert_eq!(gestures.end(1, ms(100)), Some(TouchGesture::Tap));
    }
}
