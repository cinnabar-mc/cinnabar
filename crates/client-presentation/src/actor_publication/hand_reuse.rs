//! Reuses the previous frame's first-person source while nothing it reads has changed.
//!
//! The source depends on the local actor and rig, the canonical body presentation, the
//! displayed stacks, the session's artwork and equipment, and the frame fraction. Revisions fence
//! the actor and rig, the other inputs are compared, and a source is only kept across frames when
//! the frame fraction cannot reach it: an empty bare arm whose swing and equip are not moving and
//! whose visibility gates read no frame, camera or swing input.
use super::hand::HandSource;
use crate::presentation::{actors::ActorRigPresentation, equipment::ActorEquipmentInput};
use chunk_pipeline::WorldStream;

type AnimationKey = (u32, Option<u32>, u32, bool);

/// Every input of a reusable source besides the frame fraction and the camera-space hand motion.
#[derive(Debug, PartialEq)]
pub(super) struct ReuseKey {
    revision: client_world::ActorPresentationRevision,
    presentation: ActorRigPresentation,
    hands_empty: bool,
    hand_use: (Option<u32>, Option<AnimationKey>),
    aspect: u32,
    java_mode: bool,
    /// A zero frame fraction may draw the completed tick's visibility instead of a sample.
    positive_fraction: bool,
    /// Advances whenever the session's artwork or equipment runtime may have changed.
    session: u64,
}

impl ReuseKey {
    /// Captures the inputs the source was or would be built from.
    pub(super) fn new(
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        equipment: &ActorEquipmentInput,
        (consume_ticks, item_animation): (
            Option<u32>,
            Option<actor_animation::AttachableAnimationInput<'_>>,
        ),
        (alpha, aspect, java_mode, session): (f32, f32, bool, u64),
    ) -> Option<Self> {
        let runtime_id = presentation.submission.input.identity.runtime_id;
        Some(Self {
            revision: stream.authority().actor_presentation_revision(runtime_id)?,
            presentation: presentation.clone(),
            hands_empty: equipment.main.is_none() && equipment.off.is_none(),
            hand_use: (
                consume_ticks,
                item_animation.map(|input| {
                    (
                        input.animation_frame,
                        input.use_elapsed_ticks,
                        input.max_use_ticks,
                        input.hand_charged,
                    )
                }),
            ),
            aspect: aspect.to_bits(),
            java_mode,
            positive_fraction: alpha > 0.0,
            session,
        })
    }

    /// Whether `source`, freshly built from these inputs, reads nothing that changes between
    /// frames with an equal key: no held item or map, the bare legacy arm, a still swing and
    /// equip, and visibility gates independent of the frame fraction and camera.
    pub(super) fn reusable(&self, stream: &WorldStream, source: &HandSource) -> bool {
        let runtime_id = self.presentation.submission.input.identity.runtime_id;
        self.hands_empty
            && !self.java_mode
            && source.items.iter().all(Option::is_none)
            && source.maps.paper.iter().all(Option::is_none)
            && source.body_camera.is_some()
            && stream
                .authority()
                .actor_rig(runtime_id)
                .is_some_and(|rig| rig.hand[0] == rig.hand[1])
            && !stream.authority().actor_visibility_reads_frame(runtime_id)
    }
}

/// A first-person source with the key it was built from and whether it may outlive its frame.
pub(super) struct KeyedSource {
    pub(super) key: ReuseKey,
    pub(super) reusable: bool,
}

/// The published source retained for the next frame's readiness.
#[derive(Default)]
pub(super) struct HandReuse {
    retained: Option<(ReuseKey, HandSource)>,
    /// Advances whenever the session's artwork or equipment runtime may have changed.
    pub(super) session: u64,
    /// Sources taken across frames since the state was created.
    #[cfg(test)]
    pub(super) reuses: u64,
}

impl HandReuse {
    /// Takes the retained source when it was built from `key`, with this frame's hand motion.
    pub(super) fn take(&mut self, key: &ReuseKey, motion: bevy::math::Mat4) -> Option<HandSource> {
        let (retained, source) = self.retained.take()?;
        if retained != *key {
            return None;
        }
        let source = HandSource { motion, ..source };
        #[cfg(test)]
        {
            self.reuses += 1;
        }
        Some(source)
    }

    /// Retains a published source for the next frame when it may outlive this one.
    pub(super) fn retain(&mut self, keyed: Option<KeyedSource>, source: Option<HandSource>) {
        self.retained = keyed
            .filter(|keyed| keyed.reusable)
            .zip(source)
            .map(|(keyed, source)| (keyed.key, source));
    }

    /// Forgets the retained source and any reuse in progress.
    pub(super) fn invalidate(&mut self) {
        self.retained = None;
        self.session = self.session.wrapping_add(1);
    }
}

/// Whether a reused source draws exactly what a fresh build of the same inputs draws.
#[cfg(any(test, debug_assertions))]
pub(super) fn same_output(reused: &HandSource, fresh: &HandSource) -> bool {
    reused.presentation == fresh.presentation
        && reused.visibility == fresh.visibility
        && reused.body == fresh.body
        && reused.items.iter().chain(&fresh.items).all(Option::is_none)
        && reused.maps.paper == fresh.maps.paper
        && reused.motion == fresh.motion
        && reused.body_camera == fresh.body_camera
        && reused.item_glint.as_ref().map(|glint| &glint.rgba8)
            == fresh.item_glint.as_ref().map(|glint| &glint.rgba8)
        && reused.maps.background.as_ref().map(|atlas| &atlas.rgba8)
            == fresh.maps.background.as_ref().map(|atlas| &atlas.rgba8)
}
