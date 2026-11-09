//! Read-only native body, arm and item-parent poses retained for unchanged captured frames.
use super::*;
use std::borrow::Cow;

type RenderPose = Arc<[render_model::RenderBoneTransform]>;

struct NativeBodyPose {
    bones: [RenderPose; 2],
    skin: Option<Arc<[actor_animation::SkinRenderLayer]>>,
}

#[derive(Clone, Copy)]
struct NativePoseInput {
    consume: Option<u32>,
    animation: Option<actor_animation::AttachableAnimationInput<'static>>,
    alpha: f32,
    camera: Option<([f32; 2], [f32; 3])>,
    sample_skin: bool,
    visibility_only: bool,
}

#[derive(PartialEq)]
struct PoseKey {
    source: Option<SourceKey>,
    tick: (u32, u64, u64),
    actor_alpha: u32,
    physics_alpha: Option<u32>,
    camera: Option<([f32; 2], [f32; 3])>,
    sample_skin: bool,
    visibility_only: bool,
}

impl NativePoseInput {
    /// Fences retained samples and conversions by the same captured inputs.
    fn key(self, stream: &WorldStream, runtime_id: u64) -> Option<PoseKey> {
        let rig = stream.authority().actor_rig(runtime_id)?;
        Some(PoseKey {
            source: source_key(stream, self.consume, self.animation, self.alpha),
            tick: (rig.rig.0, rig.completed_tick, rig.reset_generation),
            actor_alpha: self.alpha.to_bits(),
            physics_alpha: rig.java.local_swing_alpha.map(f32::to_bits),
            camera: self.camera,
            sample_skin: self.sample_skin,
            visibility_only: self.visibility_only,
        })
    }
}

/// Keeps one lifetime's sampled pose instead of resampling unchanged captured inputs.
#[derive(Default)]
pub(in super::super) struct NativePoseCache {
    pub(super) maps: super::maps::MapCache,
    rest: Option<(
        (
            actor_animation::ActorLifetimeId,
            actor_animation::EntityRigId,
            u64,
        ),
        RenderPose,
    )>,
    key: Option<PoseKey>,
    sample_key: Option<PoseKey>,
    sampled: Option<SampledLayers>,
    render_scratch: Vec<actor_animation::RenderTextureLayer>,
    pose: Option<[RenderPose; 2]>,
    skin_layers: Option<Arc<[actor_animation::SkinRenderLayer]>>,
    pub(in super::super) held_item_scale: Option<f32>,
    #[cfg(test)]
    pub(super) sample_work: (u64, u64),
    #[cfg(test)]
    pub(super) layer_samples: u64,
}

/// Only changed layers need ownership beyond the authority borrow.
struct SampledLayers {
    render: Option<Vec<actor_animation::RenderTextureLayer>>,
    visibility: actor_animation::RenderVisibility,
    held_item_scale: Option<f32>,
    skin: Option<Arc<[actor_animation::SkinRenderLayer]>>,
}

impl NativePoseCache {
    /// Reads visibility from the same retained sample later used by the native pose.
    pub(in super::super) fn visibility(
        &mut self,
        inputs: &HandInputs<'_>,
        java_mode: bool,
    ) -> Option<actor_animation::RenderVisibility> {
        let runtime_id = inputs.presentation.submission.input.identity.runtime_id;
        let rig = inputs.stream.authority().actor_rig(runtime_id)?;
        let visibility_only = java_mode
            || (inputs.equipment_input.main.is_none()
                && inputs.equipment_input.off.is_none()
                && inputs
                    .stream
                    .authority()
                    .actor(runtime_id)
                    .is_some_and(|actor| rig.animation_variables.uses_legacy_swing(actor)));
        self.sample_layers(
            inputs.stream,
            &inputs.presentation,
            NativePoseInput {
                consume: inputs.consume_ticks,
                animation: inputs.item_animation,
                alpha: inputs.alpha,
                camera: inputs.sampling_camera,
                sample_skin: false,
                visibility_only,
            },
        )?;
        Some(self.sampled.as_ref()?.visibility)
    }

    /// Samples once for matching captured inputs; completed slices need no retained copy.
    fn sample_layers(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        input: NativePoseInput,
    ) -> Option<()> {
        let runtime_id = presentation.submission.input.identity.runtime_id;
        let mut key = input.key(stream, runtime_id)?;
        if input.camera.is_some() {
            // A full native sample also serves a later visibility-only consumer.
            if key.visibility_only {
                key.visibility_only = false;
                if self.sample_key.as_ref() == Some(&key) {
                    return self.sampled.as_ref().map(|_| ());
                }
                key.visibility_only = true;
            }
            if self.sample_key.as_ref() == Some(&key) {
                return self.sampled.as_ref().map(|_| ());
            }
        }
        #[cfg(test)]
        {
            self.layer_samples += 1;
        }
        if let Some(render) = self
            .sampled
            .as_mut()
            .and_then(|sample| sample.render.take())
        {
            self.render_scratch = render;
        }
        self.sampled = stream
            .authority()
            .actor_render_frame(input.alpha)
            .into_layers_reusing(
                runtime_id,
                input.sample_skin,
                input.visibility_only,
                &mut self.render_scratch,
            )
            .map(|layers| SampledLayers {
                render: match layers.render {
                    Cow::Owned(render) => Some(render),
                    Cow::Borrowed(_) => None,
                },
                visibility: layers.visibility,
                held_item_scale: layers.held_item_scale,
                skin: match layers.skin {
                    Cow::Owned(skin) => Some(skin.into()),
                    Cow::Borrowed(_) => None,
                },
            });
        self.sample_key = Some(key);
        self.sampled.as_ref().map(|_| ())
    }

    /// Converts immutable rest geometry once for the bare-arm camera stack.
    pub(super) fn rest_pose(
        &mut self,
        rig: &actor_animation::ActorRigSnapshot<'_>,
    ) -> Option<RenderPose> {
        let key = (rig.actor, rig.rig, rig.rest_reset_generation);
        if self
            .rest
            .as_ref()
            .is_none_or(|(retained, _)| *retained != key)
        {
            self.rest = Some((key, crate::presentation::actors::convert_bones(rig.rest)?));
        }
        self.rest.as_ref().map(|(_, pose)| Arc::clone(pose))
    }

    /// Applies the sampled parent only when a native arm or attachable consumes its bones.
    pub(in super::super) fn apply(&mut self, inputs: &mut HandInputs<'_>) {
        if let Some([previous, current]) = self.sample(
            inputs.stream,
            &inputs.presentation,
            inputs.consume_ticks,
            inputs.item_animation,
            inputs.alpha,
            inputs.sampling_camera,
        ) {
            inputs.presentation.submission.input.previous_bones = previous;
            inputs.presentation.submission.input.current_bones = current;
        }
    }

    /// The body-only sampled persona slice; borrowed completed layers stay on the actor snapshot.
    pub(in super::super) fn skin_layers(&self) -> Option<&[actor_animation::SkinRenderLayer]> {
        self.skin_layers.as_deref()
    }

    /// Shares a body's native sample with its equipment and persona layers.
    pub(in super::super) fn apply_body(
        &mut self,
        stream: &WorldStream,
        presentation: &mut ActorRigPresentation,
        alpha: f32,
        camera: Option<([f32; 2], [f32; 3])>,
    ) {
        if let Some([previous, current]) = self.sample_with_skin(
            stream,
            presentation,
            NativePoseInput {
                consume: None,
                animation: None,
                alpha,
                camera,
                sample_skin: true,
                visibility_only: false,
            },
        ) {
            presentation.submission.input.previous_bones = previous;
            presentation.submission.input.current_bones = current;
        }
    }

    /// Publishes a remote body and returns its sampled persona layers from the shared frame.
    pub(in super::super) fn apply_frame_body(
        frame: &mut actor_animation::ActorRenderFrame<'_>,
        presentation: &mut ActorRigPresentation,
    ) -> Option<Arc<[actor_animation::SkinRenderLayer]>> {
        let runtime_id = presentation.submission.input.identity.runtime_id;
        let NativeBodyPose {
            bones: [previous, current],
            skin,
        } = body_pose(presentation, frame.layers_with_skin(runtime_id)?)?;
        presentation.submission.input.previous_bones = previous;
        presentation.submission.input.current_bones = current;
        skin
    }

    /// Samples native parent bones without requesting unused first-person persona layers.
    pub(super) fn sample(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        consume: Option<u32>,
        animation: Option<actor_animation::AttachableAnimationInput<'static>>,
        alpha: f32,
        camera: Option<([f32; 2], [f32; 3])>,
    ) -> Option<[RenderPose; 2]> {
        self.sample_with_skin(
            stream,
            presentation,
            NativePoseInput {
                consume,
                animation,
                alpha,
                camera,
                sample_skin: false,
                visibility_only: false,
            },
        )
    }

    /// Retains the exact native consumer's read-only result for unchanged captured inputs.
    fn sample_with_skin(
        &mut self,
        stream: &WorldStream,
        presentation: &ActorRigPresentation,
        input: NativePoseInput,
    ) -> Option<[RenderPose; 2]> {
        let key = input.key(stream, presentation.submission.input.identity.runtime_id)?;
        if input.camera.is_some() && self.key.as_ref() == Some(&key) {
            return self.pose.clone();
        }
        #[cfg(test)]
        let allocated = test_alloc::thread_allocations();
        self.sample_layers(stream, presentation, input)?;
        let sampled = self.sampled.as_ref()?;
        self.held_item_scale = sampled.held_item_scale;
        self.skin_layers = sampled.skin.clone();
        let pose = sampled
            .render
            .as_deref()
            .and_then(|layers| body_bones(presentation, layers));
        #[cfg(test)]
        {
            self.sample_work.0 += 1;
            self.sample_work.1 += test_alloc::thread_allocations() - allocated;
        }
        self.key = Some(key);
        self.pose = pose.clone();
        pose
    }
}

/// Converts sampled native endpoints only when they match the runtime body's skeleton.
fn body_pose(
    presentation: &ActorRigPresentation,
    sampled: actor_animation::ActorRenderLayers<'_>,
) -> Option<NativeBodyPose> {
    let Cow::Owned(layers) = sampled.render else {
        return None;
    };
    let bones = body_bones(presentation, &layers)?;
    let skin = match sampled.skin {
        Cow::Owned(skin) => Some(skin.into()),
        Cow::Borrowed(_) => None,
    };
    Some(NativeBodyPose { bones, skin })
}

/// Converts only a matching runtime body's endpoints, borrowing sampled texture layers.
fn body_bones(
    presentation: &ActorRigPresentation,
    layers: &[actor_animation::RenderTextureLayer],
) -> Option<[RenderPose; 2]> {
    let layer = layers.iter().find(|layer| {
        layer.geometry.is_none()
            && layer.pose.len() == presentation.submission.input.current_bones.len()
            && layer.previous_pose.len() == presentation.submission.input.previous_bones.len()
            && !layer.pose.is_empty()
    })?;
    let current = crate::presentation::actors::convert_bones(&layer.pose)?;
    let previous = if Arc::ptr_eq(&layer.previous_pose, &layer.pose) {
        Arc::clone(&current)
    } else {
        crate::presentation::actors::convert_bones(&layer.previous_pose)?
    };
    Some([previous, current])
}
