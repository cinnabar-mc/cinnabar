use super::motion::WalkState;
use super::{ActorTickInput, JavaMotion, LocalSwingProgress, MotionInput, MotionState, java::body};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// Resolved local motion and attack observations from one completed simulation tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalSwingMotionSample {
    pub tick: u64,
    pub delta: [f32; 3],
    pub yaw: f32,
    pub progress: LocalSwingProgress,
}

/// Local body and leg motion retain the latest physics tick's reversible step.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct State {
    authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    last: Option<LocalSwingMotionSample>,
    before: [f32; 2],
    before_walk: Option<(WalkState, [f32; 2], [f32; 2])>,
    previous_walk: Option<WalkState>,
    hurt_after_walk: bool,
    riding: bool,
    on_fire: bool,
}

impl State {
    /// Changing simulation ownership collapses the old endpoint and clears retry history.
    pub(super) fn set_authority(
        &mut self,
        authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
        motion: &mut JavaMotion,
    ) {
        if self.authority != authority {
            *self = Self {
                authority,
                ..Self::default()
            };
            motion.body_yaw = [motion.body_yaw[1]; 2];
            motion.body_frame_alpha = None;
        }
    }

    /// Physics-owned torso and leg cycles never advance on the independent actor clock.
    pub(super) fn active(&self) -> bool {
        self.authority.is_some()
    }

    /// Hurt impulses applied after the latest movement tick survive its correction replay.
    pub(super) fn note_hurt(&mut self) {
        self.hurt_after_walk = self.last.is_some();
    }

    /// Retains tick-owned posture and fire state for the local movement observations.
    pub(super) fn observe_walk(&mut self, riding: bool, on_fire: bool) {
        self.riding = riding;
        self.on_fire = on_fire;
    }

    /// Interpolates local walking on the physics fraction while other pose inputs retain their clock.
    pub(super) fn sample_walk(
        &self,
        input: ActorTickInput,
        walk: &MotionState,
        alpha: f32,
    ) -> Option<ActorTickInput> {
        let previous = self.previous_walk?;
        let alpha = render_model::legacy_combat::partial_tick(alpha);
        let sample = |from: f32, to: f32| from + (to - from) * alpha;
        Some(ActorTickInput {
            distance_moved: sample(previous.distance, walk.distance),
            move_speed: sample(previous.speed.min(1.0), walk.speed.min(1.0)),
            ..input
        })
    }

    /// Whether local gait endpoints differ or the retained actor input has yet to catch up.
    pub(super) fn walk_changed(&self, walk: &MotionState, input: ActorTickInput) -> bool {
        self.previous_walk.is_some_and(|previous| {
            previous.distance != walk.distance
                || previous.speed != walk.speed
                || input.distance_moved != walk.distance
                || input.move_speed != walk.speed.min(1.0)
        })
    }

    /// Whether physics has supplied a gait pair for this simulation identity.
    pub(super) fn has_walk(&self) -> bool {
        self.previous_walk.is_some()
    }

    /// A changed latest tick replays movement from its retained pre-tick checkpoint.
    pub(super) fn apply(
        &mut self,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        sample: LocalSwingMotionSample,
        motion: &mut JavaMotion,
        walk: &mut MotionState,
    ) -> bool {
        if self.authority != Some(authority)
            || self.last.is_some_and(|last| sample.tick < last.tick)
        {
            return false;
        }
        let previous_heading = motion.body_yaw;
        let previous_limbs = (motion.limb_swing, motion.limb_amount);
        let previous_walk = walk.walk_state();
        let advance_walk = self
            .last
            .is_none_or(|last| sample.tick != last.tick || sample.delta != last.delta);
        if let Some(last) = self.last.filter(|last| sample.tick == last.tick) {
            motion.body_frame_alpha = sample.progress.frame_alpha;
            if sample.delta == last.delta
                && sample.yaw == last.yaw
                && sample.progress.java[1] == last.progress.java[1]
            {
                return false;
            }
            motion.body_yaw = self.before;
            if advance_walk && let Some((saved, swing, amount)) = self.before_walk {
                walk.restore_walk(saved);
                motion.limb_swing = swing;
                motion.limb_amount = amount;
            }
        } else {
            self.before = motion.body_yaw;
            self.before_walk = Some((previous_walk, motion.limb_swing, motion.limb_amount));
            self.hurt_after_walk = false;
        }
        if advance_walk {
            self.previous_walk = Some(walk.walk_state());
            walk.advance_walk(&MotionInput {
                delta: sample.delta,
                riding: self.riding,
                player: true,
                yaw: sample.yaw,
                head_yaw: sample.yaw,
                on_fire: self.on_fire,
                actor_walk: false,
            });
            motion.advance_limbs(sample.delta);
            if self.hurt_after_walk {
                motion.hurt_limbs();
            }
        }
        body::advance(
            &mut motion.body_yaw,
            sample.delta,
            sample.yaw,
            sample.progress.java[1],
        );
        motion.body_frame_alpha = sample.progress.frame_alpha;
        self.last = Some(sample);
        motion.body_yaw != previous_heading
            || (motion.limb_swing, motion.limb_amount) != previous_limbs
            || walk.walk_state() != previous_walk
    }
}

impl super::ActorAnimationStore {
    /// Binds local torso and leg ticks to the simulation, including a rig created after binding.
    pub fn set_local_motion_authority(
        &mut self,
        runtime_id: u64,
        authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    ) {
        if let Some((previous, _)) = self.local_motion_authority
            && previous != runtime_id
            && let Some(state) = self
                .runtime_to_lifetime
                .get(&previous)
                .and_then(|lifetime| self.rigs.get_mut(lifetime))
        {
            state
                .java
                .local_body
                .set_authority(None, &mut state.java.motion);
        }
        self.local_motion_authority = authority.map(|identity| (runtime_id, identity));
        if let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|lifetime| self.rigs.get_mut(lifetime))
        {
            state
                .java
                .local_body
                .set_authority(authority, &mut state.java.motion);
        }
    }

    /// Applies local torso and leg motion once per physics tick, retaining action and cape clocks.
    pub fn sync_local_swing_motion(
        &mut self,
        runtime_id: u64,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        samples: impl IntoIterator<Item = LocalSwingMotionSample>,
    ) -> bool {
        if self.local_motion_authority != Some((runtime_id, authority)) {
            return false;
        }
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|lifetime| self.rigs.get_mut(lifetime))
        else {
            return false;
        };
        let mut changed = false;
        for sample in samples {
            changed |= state.java.local_body.apply(
                authority,
                sample,
                &mut state.java.motion,
                &mut state.motion,
            );
        }
        changed
    }
}
