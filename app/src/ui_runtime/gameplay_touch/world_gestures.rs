//! Touch and classic modes: a finger on the world taps, holds or turns the camera.

use std::time::Duration;

use semantic_input::{TouchContact, TouchGesture, TouchGestures, touch};

/// What a finger on the world points at, which selects the action it performs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorldTarget {
    Actor,
    Block,
    Nothing,
}

impl WorldTarget {
    /// A tap attacks an actor and otherwise uses the held item on what it points at.
    const fn tap_action(self) -> u16 {
        match self {
            Self::Actor => touch::ATTACK,
            Self::Block | Self::Nothing => touch::USE,
        }
    }

    /// A hold breaks a block and otherwise interacts with what it points at.
    const fn hold_action(self) -> u16 {
        match self {
            Self::Block => touch::ATTACK,
            Self::Actor | Self::Nothing => touch::USE,
        }
    }
}

/// Gesture state of the fingers on the world, kept between frames.
#[derive(Debug, Default)]
pub(crate) struct WorldTouches {
    gestures: TouchGestures,
    /// The action each holding finger performs, fixed when its hold starts.
    holds: Vec<(u64, u16)>,
}

/// The interactions one frame of world touches produced.
#[derive(Debug, Default, PartialEq)]
pub(super) struct WorldFrame {
    /// Actions a finger newly pressed.
    pub(super) pressed: Vec<u16>,
    /// Tap actions to route once.
    pub(super) pulses: Vec<TouchContact>,
    /// Where a finger points at the world this frame, in window fractions.
    pub(super) point: Option<[f32; 2]>,
}

impl WorldTouches {
    /// Classifies this frame's world fingers. A finger that is not turning stops looking, and a
    /// holding finger's sample becomes its action. `size` is the window in pixels.
    pub(super) fn apply(
        &mut self,
        held: &mut [TouchContact],
        lifted: &mut Vec<TouchContact>,
        now: Duration,
        size: [f32; 2],
        target: WorldTarget,
    ) -> WorldFrame {
        let mut frame = WorldFrame::default();
        lifted.retain_mut(|sample| {
            if sample.hit_id != Some(touch::LOOK_SURFACE) {
                return true;
            }
            let (turning, _) = self.advance(sample, now, size);
            let gesture = self.gestures.end(sample.contact_id, now);
            self.holds.retain(|(id, _)| *id != sample.contact_id);
            if gesture == Some(TouchGesture::Tap) {
                let action = target.tap_action();
                frame.pressed.push(action);
                frame.point = Some(sample.position);
                frame.pulses.push(TouchContact {
                    hit_id: Some(action),
                    delta: [0.0; 2],
                    ..sample.clone()
                });
            }
            turning
        });
        for sample in held
            .iter_mut()
            .filter(|sample| sample.hit_id == Some(touch::LOOK_SURFACE))
        {
            let (turning, gesture) = self.advance(sample, now, size);
            if !turning {
                sample.delta = [0.0; 2];
            }
            match gesture {
                Some(TouchGesture::HoldStarted) => {
                    let action = target.hold_action();
                    self.holds.push((sample.contact_id, action));
                    frame.pressed.push(action);
                }
                Some(TouchGesture::HoldEnded) => {
                    self.holds.retain(|(id, _)| *id != sample.contact_id);
                }
                _ => {}
            }
            frame.point = Some(sample.position);
            if let Some((_, action)) = self.holds.iter().find(|(id, _)| *id == sample.contact_id) {
                sample.hit_id = Some(*action);
            }
        }
        let is_held = |id: u64| held.iter().any(|sample| sample.contact_id == id);
        self.gestures.retain(is_held);
        self.holds.retain(|(id, _)| is_held(*id));
        frame
    }

    /// Forgets every finger without completing a gesture, as when gameplay loses input.
    pub(super) fn clear(&mut self) {
        self.gestures.clear();
        self.holds.clear();
    }

    /// Advances one finger by its sampled movement, measured in screen widths.
    fn advance(
        &mut self,
        sample: &TouchContact,
        now: Duration,
        size: [f32; 2],
    ) -> (bool, Option<TouchGesture>) {
        if !self.gestures.contains(sample.contact_id) {
            self.gestures.begin(sample.contact_id, now);
        }
        let step = sample.delta[0].hypot(sample.delta[1] * size[1] / size[0]);
        self.gestures.advance(sample.contact_id, step, size[0], now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [f32; 2] = [1000.0, 500.0];

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn finger(delta: [f32; 2]) -> TouchContact {
        TouchContact {
            contact_id: 4,
            activity_sequence: 0,
            position: [0.6, 0.4],
            delta,
            hit_id: Some(touch::LOOK_SURFACE),
        }
    }

    /// Runs one frame with the finger still down.
    fn down(
        world: &mut WorldTouches,
        delta: [f32; 2],
        at: u64,
        target: WorldTarget,
    ) -> (WorldFrame, TouchContact) {
        let mut held = [finger(delta)];
        let frame = world.apply(&mut held, &mut Vec::new(), ms(at), SIZE, target);
        let [sample] = held;
        (frame, sample)
    }

    /// Runs one frame in which the finger lifts, returning the samples still routed.
    fn up(
        world: &mut WorldTouches,
        at: u64,
        target: WorldTarget,
    ) -> (WorldFrame, Vec<TouchContact>) {
        let mut lifted = vec![finger([0.0; 2])];
        let frame = world.apply(&mut [], &mut lifted, ms(at), SIZE, target);
        (frame, lifted)
    }

    #[test]
    fn a_tap_attacks_an_actor_and_uses_anything_else() {
        for (target, action) in [
            (WorldTarget::Actor, touch::ATTACK),
            (WorldTarget::Block, touch::USE),
            (WorldTarget::Nothing, touch::USE),
        ] {
            let mut world = WorldTouches::default();
            let (pressed, _) = down(&mut world, [0.0; 2], 0, target);
            assert!(pressed.pressed.is_empty());
            let (frame, routed) = up(&mut world, 100, target);
            assert_eq!(frame.pressed, [action]);
            assert_eq!(frame.pulses[0].hit_id, Some(action));
            assert_eq!(frame.point, Some([0.6, 0.4]));
            assert!(routed.is_empty(), "the look sample never routes");
        }
    }

    #[test]
    fn a_tap_shorter_than_a_frame_still_lands() {
        let mut world = WorldTouches::default();
        let (frame, _) = up(&mut world, 0, WorldTarget::Block);
        assert_eq!(frame.pressed, [touch::USE]);
    }

    #[test]
    fn a_hold_breaks_a_block_and_interacts_with_anything_else() {
        for (target, action) in [
            (WorldTarget::Block, touch::ATTACK),
            (WorldTarget::Actor, touch::USE),
            (WorldTarget::Nothing, touch::USE),
        ] {
            let mut world = WorldTouches::default();
            down(&mut world, [0.0; 2], 0, target);
            let (frame, sample) = down(&mut world, [0.0; 2], 400, target);
            assert_eq!(frame.pressed, [action]);
            assert_eq!(sample.hit_id, Some(action));
            let (frame, sample) = down(&mut world, [0.0; 2], 500, target);
            assert!(frame.pressed.is_empty(), "a hold presses once");
            assert_eq!(sample.hit_id, Some(action));
            let (frame, routed) = up(&mut world, 900, target);
            assert_eq!(frame, WorldFrame::default());
            assert!(routed.is_empty());
        }
    }

    #[test]
    fn a_hold_keeps_its_action_when_the_target_changes() {
        let mut world = WorldTouches::default();
        down(&mut world, [0.0; 2], 0, WorldTarget::Block);
        down(&mut world, [0.0; 2], 400, WorldTarget::Block);
        let (_, sample) = down(&mut world, [0.0; 2], 500, WorldTarget::Nothing);
        assert_eq!(sample.hit_id, Some(touch::ATTACK));
    }

    #[test]
    fn dragging_looks_only_after_the_threshold_and_ends_a_hold() {
        let mut world = WorldTouches::default();
        let (_, sample) = down(&mut world, [0.02, 0.0], 0, WorldTarget::Block);
        assert_eq!(sample.delta, [0.0; 2], "below the threshold");
        let (_, sample) = down(&mut world, [0.04, 0.0], 16, WorldTarget::Block);
        assert_eq!(sample.delta, [0.04, 0.0]);
        assert_eq!(sample.hit_id, Some(touch::LOOK_SURFACE));
        let (frame, routed) = up(&mut world, 100, WorldTarget::Block);
        assert!(frame.pulses.is_empty(), "a drag never taps");
        assert_eq!(
            routed.len(),
            1,
            "a turning finger's last motion still looks"
        );

        let mut world = WorldTouches::default();
        down(&mut world, [0.0; 2], 0, WorldTarget::Block);
        down(&mut world, [0.0; 2], 400, WorldTarget::Block);
        let (_, sample) = down(&mut world, [0.06, 0.0], 500, WorldTarget::Block);
        assert_eq!(sample.hit_id, Some(touch::LOOK_SURFACE), "the hold ended");
    }

    /// Vertical travel counts in screen widths, so a tall screen does not turn sooner.
    #[test]
    fn vertical_travel_is_measured_in_screen_widths() {
        let mut world = WorldTouches::default();
        let (_, sample) = down(&mut world, [0.0, 0.08], 0, WorldTarget::Nothing);
        assert_eq!(
            sample.delta, [0.0; 2],
            "0.08 of the height is 0.04 of the width"
        );
    }

    #[test]
    fn a_cancelled_finger_is_forgotten() {
        let mut world = WorldTouches::default();
        down(&mut world, [0.0; 2], 0, WorldTarget::Block);
        world.apply(&mut [], &mut Vec::new(), ms(16), SIZE, WorldTarget::Block);
        let (frame, _) = down(&mut world, [0.0; 2], 500, WorldTarget::Block);
        assert!(
            frame.pressed.is_empty(),
            "the reused id starts a new gesture"
        );
    }
}
