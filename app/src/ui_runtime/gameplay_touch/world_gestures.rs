//! Touch and classic modes: a finger on the world taps, holds or turns the camera.

use std::{collections::VecDeque, time::Duration};

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

/// Frames a finger's ray must be published before the pick reflects it. Touch input runs ahead of
/// the frame's pick, and interactions read the previous frame's published ray.
pub(crate) const RAY_SETTLE_FRAMES: u8 = 2;

/// Marks a tap's routed contact, whose finger's id may already belong to a new press.
const TAP_CONTACT_BIT: u64 = 1 << 63;

/// A completed tap waiting for its ray to reach the pick.
#[derive(Debug)]
struct PendingTap {
    sample: TouchContact,
    ray_frames: u8,
}

/// A holding finger. Its action is chosen once the pick has reached the ray it started on.
#[derive(Debug)]
struct Hold {
    contact_id: u64,
    action: Option<u16>,
    aim: [f32; 2],
    ray_frames: u8,
}

/// The ray a held finger last published and how many frames it has stayed there.
#[derive(Debug)]
struct FingerRay {
    contact_id: u64,
    position: [f32; 2],
    frames: u8,
}

/// Gesture state of the fingers on the world, kept between frames.
#[derive(Debug, Default)]
pub(crate) struct WorldTouches {
    gestures: TouchGestures,
    holds: Vec<Hold>,
    rays: Vec<FingerRay>,
    /// Completed taps in order; only the first aims the ray, so each settles in turn.
    pending: VecDeque<PendingTap>,
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
    /// holding finger's sample becomes its action. A tap acts once the pick has caught up with its
    /// ray, keeping that ray aimed until then. `size` is the window in pixels.
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
            if gesture == Some(TouchGesture::Tap) {
                self.pending.push_back(PendingTap {
                    sample: sample.clone(),
                    ray_frames: self.ray_frames(sample.contact_id, sample.position),
                });
            }
            self.forget_finger(sample.contact_id);
            turning
        });
        if let Some(tap) = self.pending.front_mut() {
            frame.point = Some(tap.sample.position);
            if tap.ray_frames < RAY_SETTLE_FRAMES {
                tap.ray_frames += 1;
            } else if let Some(tap) = self.pending.pop_front() {
                let action = target.tap_action();
                frame.pressed.push(action);
                frame.pulses.push(TouchContact {
                    contact_id: tap.sample.contact_id | TAP_CONTACT_BIT,
                    hit_id: Some(action),
                    delta: [0.0; 2],
                    ..tap.sample
                });
            }
        }
        for sample in held
            .iter_mut()
            .filter(|sample| sample.hit_id == Some(touch::LOOK_SURFACE))
        {
            let (turning, gesture) = self.advance(sample, now, size);
            if !turning {
                sample.delta = [0.0; 2];
            }
            match gesture {
                Some(TouchGesture::HoldStarted) => self.holds.push(Hold {
                    contact_id: sample.contact_id,
                    action: None,
                    aim: sample.position,
                    ray_frames: self.ray_frames(sample.contact_id, sample.position),
                }),
                Some(TouchGesture::HoldEnded) => {
                    self.holds
                        .retain(|hold| hold.contact_id != sample.contact_id);
                }
                _ => {}
            }
            let hold = self
                .holds
                .iter_mut()
                .find(|hold| hold.contact_id == sample.contact_id);
            match hold {
                Some(hold) if hold.action.is_none() => {
                    if frame.point.is_none() {
                        frame.point = Some(hold.aim);
                        if hold.ray_frames < RAY_SETTLE_FRAMES {
                            hold.ray_frames += 1;
                        } else {
                            let action = target.hold_action();
                            hold.action = Some(action);
                            frame.pressed.push(action);
                        }
                    }
                }
                _ => {
                    if frame.point.is_none() {
                        frame.point = Some(sample.position);
                        self.note_ray(sample.contact_id, sample.position);
                    }
                }
            }
            if let Some(action) = self
                .holds
                .iter()
                .find(|hold| hold.contact_id == sample.contact_id)
                .and_then(|hold| hold.action)
            {
                sample.hit_id = Some(action);
            }
        }
        let is_held = |id: u64| held.iter().any(|sample| sample.contact_id == id);
        self.gestures.retain(is_held);
        self.holds.retain(|hold| is_held(hold.contact_id));
        self.rays.retain(|ray| is_held(ray.contact_id));
        frame
    }

    /// Forgets a cancelled finger before its id can start a new press in the same frame.
    pub(super) fn cancel(&mut self, contact_id: u64) {
        self.forget_finger(contact_id);
    }

    /// Forgets every finger and waiting tap without completing a gesture, as when gameplay loses
    /// input.
    pub(super) fn clear(&mut self) {
        self.gestures.clear();
        self.holds.clear();
        self.rays.clear();
        self.pending.clear();
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

    /// Frames this finger's ray has been published at `position`; moving starts over.
    fn ray_frames(&self, contact_id: u64, position: [f32; 2]) -> u8 {
        self.rays
            .iter()
            .find(|ray| ray.contact_id == contact_id && ray.position == position)
            .map_or(0, |ray| ray.frames)
    }

    /// Counts a frame that published this finger's ray at `position`.
    fn note_ray(&mut self, contact_id: u64, position: [f32; 2]) {
        match self
            .rays
            .iter_mut()
            .find(|ray| ray.contact_id == contact_id)
        {
            Some(ray) if ray.position == position => {
                ray.frames = ray.frames.saturating_add(1).min(RAY_SETTLE_FRAMES);
            }
            Some(ray) => {
                ray.position = position;
                ray.frames = 1;
            }
            None => self.rays.push(FingerRay {
                contact_id,
                position,
                frames: 1,
            }),
        }
    }

    fn forget_finger(&mut self, contact_id: u64) {
        self.gestures.retain(|id| id != contact_id);
        self.holds.retain(|hold| hold.contact_id != contact_id);
        self.rays.retain(|ray| ray.contact_id != contact_id);
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

    /// Publishes the finger's ray for long enough that the pick reflects it.
    fn settle(world: &mut WorldTouches, target: WorldTarget) {
        for frame in 0..u64::from(RAY_SETTLE_FRAMES) {
            down(world, [0.0; 2], frame, target);
        }
    }

    #[test]
    fn a_tap_attacks_an_actor_and_uses_anything_else() {
        for (target, action) in [
            (WorldTarget::Actor, touch::ATTACK),
            (WorldTarget::Block, touch::USE),
            (WorldTarget::Nothing, touch::USE),
        ] {
            let mut world = WorldTouches::default();
            settle(&mut world, target);
            let (frame, routed) = up(&mut world, 100, target);
            assert_eq!(frame.pressed, [action]);
            assert_eq!(frame.pulses[0].hit_id, Some(action));
            assert_eq!(frame.point, Some([0.6, 0.4]));
            assert!(routed.is_empty(), "the look sample never routes");
        }
    }

    /// Runs one frame with no finger on the world.
    fn idle(world: &mut WorldTouches, at: u64, target: WorldTarget) -> WorldFrame {
        world.apply(&mut [], &mut Vec::new(), ms(at), SIZE, target)
    }

    /// A tap too short for the pick to see its ray keeps that ray aimed and acts on what the pick
    /// finds there, not on what the previous ray found.
    #[test]
    fn a_short_tap_waits_for_the_pick_to_reach_its_ray() {
        // Lifted in the frame it pressed.
        let mut world = WorldTouches::default();
        let (frame, _) = up(&mut world, 0, WorldTarget::Block);
        assert!(frame.pulses.is_empty());
        assert_eq!(frame.point, Some([0.6, 0.4]));
        let frame = idle(&mut world, 16, WorldTarget::Block);
        assert!(frame.pulses.is_empty());
        assert_eq!(frame.point, Some([0.6, 0.4]));
        let frame = idle(&mut world, 33, WorldTarget::Actor);
        assert_eq!(frame.pressed, [touch::ATTACK]);
        assert_eq!(frame.point, Some([0.6, 0.4]));
        assert_eq!(
            idle(&mut world, 50, WorldTarget::Actor),
            WorldFrame::default()
        );

        // Lifted one frame after it pressed.
        let mut world = WorldTouches::default();
        down(&mut world, [0.0; 2], 0, WorldTarget::Block);
        let (frame, _) = up(&mut world, 16, WorldTarget::Block);
        assert!(frame.pulses.is_empty());
        let frame = idle(&mut world, 33, WorldTarget::Actor);
        assert_eq!(frame.pressed, [touch::ATTACK]);
    }

    /// A waiting tap keeps the ray until it acts, so a new finger neither redirects it nor counts
    /// frames its own ray was not published.
    #[test]
    fn a_new_finger_does_not_redirect_a_waiting_tap() {
        let mut world = WorldTouches::default();
        up(&mut world, 0, WorldTarget::Block);
        let mut next = TouchContact {
            position: [0.1, 0.1],
            ..finger([0.0; 2])
        };
        for at in [16, 33] {
            let frame = world.apply(
                std::slice::from_mut(&mut next),
                &mut Vec::new(),
                ms(at),
                SIZE,
                WorldTarget::Actor,
            );
            assert_eq!(frame.point, Some([0.6, 0.4]));
        }
        assert_eq!(world.ray_frames(4, [0.1, 0.1]), 0);
    }

    /// A tap's routed contact cannot collide with a new finger reusing its id, which would reject
    /// the whole device frame.
    #[test]
    fn a_tap_keeps_a_separate_contact_from_a_reused_id() {
        let mut world = WorldTouches::default();
        settle(&mut world, WorldTarget::Block);
        let mut held = [finger([0.0; 2])];
        let mut lifted = vec![finger([0.0; 2])];
        let frame = world.apply(&mut held, &mut lifted, ms(100), SIZE, WorldTarget::Block);
        assert_eq!(frame.pulses.len(), 1);
        assert_ne!(frame.pulses[0].contact_id, held[0].contact_id);
    }

    /// Cancellation ends a gesture even when its id presses again in the same frame.
    #[test]
    fn a_cancelled_finger_passes_nothing_to_its_reused_id() {
        // A hold's action does not carry over.
        let mut world = WorldTouches::default();
        settle(&mut world, WorldTarget::Block);
        down(&mut world, [0.0; 2], 400, WorldTarget::Block);
        world.cancel(4);
        let (frame, sample) = down(&mut world, [0.0; 2], 416, WorldTarget::Block);
        assert!(frame.pressed.is_empty());
        assert_eq!(sample.hit_id, Some(touch::LOOK_SURFACE));
        // Nor does a drag: the new touch can still tap.
        let mut world = WorldTouches::default();
        down(&mut world, [0.06, 0.0], 0, WorldTarget::Block);
        world.cancel(4);
        let (_, sample) = down(&mut world, [0.01, 0.0], 16, WorldTarget::Block);
        assert_eq!(sample.delta, [0.0; 2], "the new touch is not turning");
        down(&mut world, [0.0; 2], 33, WorldTarget::Block);
        let (frame, _) = up(&mut world, 50, WorldTarget::Block);
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
            settle(&mut world, target);
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
        settle(&mut world, WorldTarget::Block);
        down(&mut world, [0.0; 2], 400, WorldTarget::Block);
        let (_, sample) = down(&mut world, [0.0; 2], 500, WorldTarget::Nothing);
        assert_eq!(sample.hit_id, Some(touch::ATTACK));
    }

    /// `finger` at another position, as when it drifts without reaching the turn threshold.
    fn moved(position: [f32; 2]) -> TouchContact {
        TouchContact {
            position,
            ..finger([0.0; 2])
        }
    }

    /// A finger that drifts onto another target and lifts acts on where it ended, not on what the
    /// pick found where it had been resting.
    #[test]
    fn a_drifting_tap_settles_on_its_final_ray() {
        let mut world = WorldTouches::default();
        settle(&mut world, WorldTarget::Block);
        let end = [0.62, 0.4];
        let mut lifted = vec![moved(end)];
        let frame = world.apply(&mut [], &mut lifted, ms(100), SIZE, WorldTarget::Block);
        assert!(frame.pulses.is_empty(), "the old ray's target must not act");
        assert_eq!(frame.point, Some(end));
        idle(&mut world, 116, WorldTarget::Block);
        let frame = idle(&mut world, 133, WorldTarget::Actor);
        assert_eq!(frame.pressed, [touch::ATTACK]);
        assert_eq!(frame.point, Some(end));
    }

    /// A hold that starts while the finger drifts freezes its aim until the pick reaches that
    /// ray, then follows the finger again.
    #[test]
    fn a_drifting_hold_chooses_its_action_on_the_ray_it_started_on() {
        let mut world = WorldTouches::default();
        settle(&mut world, WorldTarget::Block);
        let start = [0.62, 0.4];
        let mut held = [moved(start)];
        let frame = world.apply(
            &mut held,
            &mut Vec::new(),
            ms(400),
            SIZE,
            WorldTarget::Block,
        );
        assert!(
            frame.pressed.is_empty(),
            "the old ray's target must not act"
        );
        assert_eq!(frame.point, Some(start));
        let mut held = [moved([0.63, 0.4])];
        let frame = world.apply(
            &mut held,
            &mut Vec::new(),
            ms(416),
            SIZE,
            WorldTarget::Block,
        );
        assert!(frame.pressed.is_empty());
        assert_eq!(
            frame.point,
            Some(start),
            "the aim stays on the starting ray"
        );
        let mut held = [moved([0.64, 0.4])];
        let frame = world.apply(
            &mut held,
            &mut Vec::new(),
            ms(433),
            SIZE,
            WorldTarget::Actor,
        );
        assert_eq!(frame.pressed, [touch::USE]);
        assert_eq!(held[0].hit_id, Some(touch::USE));
        let mut held = [moved([0.645, 0.4])];
        let frame = world.apply(
            &mut held,
            &mut Vec::new(),
            ms(450),
            SIZE,
            WorldTarget::Actor,
        );
        assert_eq!(
            frame.point,
            Some([0.645, 0.4]),
            "then it follows the finger"
        );
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
            idle(&mut world, 116, WorldTarget::Block),
            WorldFrame::default()
        );
        assert_eq!(
            routed.len(),
            1,
            "a turning finger's last motion still looks"
        );

        let mut world = WorldTouches::default();
        settle(&mut world, WorldTarget::Block);
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
        idle(&mut world, 16, WorldTarget::Block);
        let (frame, _) = down(&mut world, [0.0; 2], 500, WorldTarget::Block);
        assert!(
            frame.pressed.is_empty(),
            "the reused id starts a new gesture"
        );
    }
}
