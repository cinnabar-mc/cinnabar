//! Publishes the captured actor batch after authoritative input has been sent.
use super::*;
use crate::presentation::actors::{ActorPresentationBatch, update_actor_rig_scene};
use render_scene::{ActorMainWitness, ActorRenderFrame, ActorRuntimeWitness};

#[derive(Resource, Default)]
pub struct PreparedActorPublication(pub(super) Option<PendingActorPublication>);

impl PreparedActorPublication {
    /// Appends client-owned entities after their normal controller and artwork resolution.
    pub fn extend_entities(&mut self, batch: ActorPresentationBatch) {
        if let Some(pending) = &mut self.0 {
            if !batch.skin_layers.is_empty() {
                return;
            }
            pending.batch.submissions.extend(batch.submissions);
            pending.batch.artwork.extend(batch.artwork);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn submissions(&self) -> Option<&[render_model::ActorRigSubmission]> {
        self.0
            .as_ref()
            .map(|pending| pending.batch.submissions.as_slice())
    }
}

pub(super) struct PendingActorPublication {
    pub(super) batch: ActorPresentationBatch,
    pub(super) mask_only_runtime_ids: Vec<u64>,
    pub(super) independent_layers: Vec<(u64, u8)>,
    pub(super) session_id: Option<u64>,
    pub(super) partial_tick: f32,
    pub(super) witness: ActorMainWitness,
}

impl PendingActorPublication {
    /// Records the pre-layer body count and captured visibility for final publication.
    pub(super) fn new(
        batch: ActorPresentationBatch,
        partial_tick: f32,
        visibility: Option<crate::local_player::FrozenLocalAvatarVisibility>,
        local_visible: bool,
        local_actor: (Option<u64>, u64),
        selected_count: usize,
        unrigged_actors: usize,
    ) -> Self {
        let (session_id, local_runtime_id) = local_actor;
        Self {
            mask_only_runtime_ids: Vec::new(),
            independent_layers: Vec::new(),
            batch,
            session_id,
            partial_tick,
            witness: ActorMainWitness {
                local_snapshot: visibility.is_some(),
                local_visible,
                expected_runtime_id: local_runtime_id,
                visibility_runtime_id: visibility.map_or(0, |snapshot| snapshot.runtime_id()),
                selected_count,
                local_route: None,
                frame_instances: 0,
                frame_manifest: 0,
                skin_bytes: 0,
                rejects: Default::default(),
                unrigged_actors,
            },
        }
    }
}

/// Builds GPU payloads from the same captured inputs that preceded the network send.
pub fn publish_actor_render_frame(
    mut prepared: ResMut<PreparedActorPublication>,
    mut scene: ResMut<ActorRenderScene>,
    mut frame: ResMut<ActorRenderFrame>,
    witness: Res<ActorRuntimeWitness>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
) {
    let Some(mut prepared) = prepared.0.take() else {
        return;
    };
    let _publication = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorPublication));
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorRigBuild));
    update_actor_rig_scene(&mut scene, prepared.partial_tick, prepared.batch);
    let published =
        scene.set_mask_only_except(prepared.mask_only_runtime_ids, &prepared.independent_layers);
    // Assigning marks the frame changed for extraction and every render-side consumer, so an
    // equal frame that draws nothing stays as it was.
    if !frame.same_empty_frame(published) {
        *frame = published.clone();
    }
    prepared.witness.local_route = frame
        .rig
        .manifest
        .iter()
        .find(|entry| {
            Some(entry.identity.session_id) == prepared.session_id
                && entry.identity.runtime_id == prepared.witness.expected_runtime_id
        })
        .map(|entry| entry.route);
    prepared.witness.frame_instances = frame.rig.instances.len();
    prepared.witness.frame_manifest = frame.rig.manifest.len();
    prepared.witness.skin_bytes = frame.skin_bytes();
    prepared.witness.rejects = frame.rig.rejects;
    witness.observe_main(prepared.witness);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{DetectChanges, World};

    /// Publishes one empty batch at `partial_tick` and reports whether the frame was assigned.
    fn publish_empty(world: &mut World, partial_tick: f32) -> bool {
        world.insert_resource(PreparedActorPublication(Some(
            PendingActorPublication::new(
                ActorPresentationBatch {
                    submissions: Vec::new(),
                    skin_layers: Vec::new(),
                    artwork: Default::default(),
                },
                partial_tick,
                None,
                false,
                (Some(1), 1),
                0,
                0,
            ),
        )));
        let before = world.resource_ref::<ActorRenderFrame>().last_changed();
        world.increment_change_tick();
        world.run_system_cached(publish_actor_render_frame).unwrap();
        world.resource_ref::<ActorRenderFrame>().last_changed() != before
    }

    /// The first empty frame is published; later equal empty frames leave the published frame,
    /// and so extraction and the renderer's change checks, untouched at any partial tick.
    #[test]
    fn equal_empty_frames_leave_the_published_frame_unchanged() {
        let mut world = World::new();
        world.init_resource::<ActorRenderScene>();
        world.init_resource::<ActorRenderFrame>();
        world.init_resource::<ActorRuntimeWitness>();
        assert!(
            publish_empty(&mut world, 0.25),
            "the first frame is published"
        );
        for partial_tick in [0.5, 0.75, 0.0] {
            assert!(
                !publish_empty(&mut world, partial_tick),
                "an equal empty frame is not republished"
            );
        }
        assert!(
            world
                .resource::<ActorRenderFrame>()
                .same_empty_frame(world.resource::<ActorRenderScene>().frame()),
            "the kept frame matches the scene's latest empty frame"
        );
    }
}
