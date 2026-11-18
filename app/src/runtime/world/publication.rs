//! Publishes completed world work and records terrain upload acknowledgements.

use super::{
    AppWorldState, WorldStreamFramePoll, flush_sub_chunk_requests, local_retention,
    sub_chunk_request_center, synchronize_biome_tints, synchronize_terrain_generation,
};
use crate::runtime::{
    network::resource_packs::PackApplication,
    shutdown::record_fatal_error,
    visibility::{CaveVisibilityCache, DiagnosticQuads},
};
use bevy::{
    log::info,
    prelude::{DetectChangesMut, Local, Res, ResMut, Time, Vec3},
    time::Virtual,
};
use chunk_pipeline::{WorldMeshChange, WorldStream};
use client_presentation::local_player::LocalViewPose;
use client_runtime::{
    ingress::OUTBOUND_SEND_BUDGET_PER_FRAME,
    network::NetworkHandle,
    publication::{PublicationController, PublicationFrameWork},
};
use render_scene::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadPriority,
    ChunkUploadToken, RuntimeStage, RuntimeStageProfiler,
};
#[cfg(feature = "acceptance")]
use {
    crate::runtime::visibility::AppMetrics, acceptance::AcceptanceRun,
    acceptance::mutation::deterministic_mutation_coordinate,
};

const MISSING_PUBLICATION_PERMIT_ERROR: &str =
    "world stream produced a render update without the required publication permit";

/// Returns whether a mesh update carries its required publication permit.
pub(crate) fn mesh_change_has_publication_permit(change: &WorldMeshChange) -> bool {
    match change {
        WorldMeshChange::Upsert { permit, .. } | WorldMeshChange::Remove { permit, .. } => {
            permit.is_some()
        }
    }
}

/// Publishes ready terrain, handles upload acknowledgements and anchors resolved spawns.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_world_stream(
    network: Res<NetworkHandle<PackApplication>>,
    state: AppWorldState,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    #[cfg(feature = "acceptance")] mut metrics: ResMut<AppMetrics>,
    mut render_queue: ResMut<ChunkRenderQueue>,
    mut biome_tints: ResMut<ChunkBiomeTints>,
    mut diagnostic_quads: ResMut<DiagnosticQuads>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    mut publication: ResMut<PublicationController>,
    mut view: ResMut<LocalViewPose>,
    mut frame_poll: ResMut<WorldStreamFramePoll>,
    mut rendered_session: Local<Option<(u64, u64)>>,
    mut visibility: ResMut<CaveVisibilityCache>,
    camera_publication: Option<
        Res<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>,
    >,
    profiler: Option<Res<RuntimeStageProfiler>>,
    (frame, mut block_use, simulation_time): (
        Res<bevy::diagnostic::FrameCount>,
        ResMut<client_interaction::block_use::BlockUseRuntime>,
        Res<Time<Virtual>>,
    ),
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldStream));
    let AppWorldState {
        mut client_world,
        mut fatal_error,
        mut local_physics,
        mut movement,
        mut ui_runtime,
        clock,
        time,
        ..
    } = state;
    let active_session = client_world
        .stream
        .as_ref()
        .map(WorldStream::terrain_generation);
    if synchronize_terrain_generation(
        &mut rendered_session,
        active_session,
        &mut render_queue,
        &acknowledgements,
    ) {
        *visibility = CaveVisibilityCache::default();
        diagnostic_quads.0.clear();
    }
    let Some(stream) = client_world.stream.as_mut() else {
        ui_runtime.clear_disconnected_block_cracks();
        return;
    };
    let prediction_budget = block_use.prediction_frames.publication_budget(frame.0);
    if prediction_budget != 0 {
        let report =
            stream.poll_prediction_jobs(view.eye_translation().to_array(), prediction_budget);
        frame_poll.report.light_results += report.light_results;
        frame_poll.report.light_jobs_dispatched += report.light_jobs_dispatched;
        frame_poll.report.mesh_results += report.mesh_results;
        frame_poll.report.mesh_jobs_dispatched += report.mesh_jobs_dispatched;
    }
    let profile = bridge::join_profile::JoinProfile::current();
    use bridge::join_profile::Milestone;
    if frame_poll.report.decoded_results > 0 {
        profile.mark_once(Milestone::ChunkDecoded);
    }
    if frame_poll.report.light_results > 0 {
        profile.mark_once(Milestone::LightingReady);
    }
    if frame_poll.report.mesh_results > 0 {
        profile.mark_once(Milestone::MeshReady);
    }
    local_retention::retain_completed_player_terrain(
        stream,
        &local_physics,
        camera_publication.as_deref(),
        clock.session_generation(),
    );
    // Marked changed only on a new table, so unchanged frames do not re-extract it.
    if synchronize_biome_tints(stream, biome_tints.bypass_change_detection()) {
        biome_tints.set_changed();
    }
    #[cfg(feature = "acceptance")]
    let mutation_cohort = frame_poll.cohort;
    for acknowledgement in acknowledgements.drain() {
        profile.mark_once(Milestone::MeshUploaded);
        block_use.prediction_frames.uploaded(
            frame.0,
            acknowledgement.key,
            acknowledgement.token.generation,
        );
        render_queue.record_gpu_upload_bytes(acknowledgement.uploaded_bytes);
        if let Some(trace) = bridge::latency_trace::active() {
            trace.record(
                bridge::latency_trace::kind::MESH_UPLOADED,
                &[
                    trace.at(acknowledgement.token.dirty_since),
                    trace.at(acknowledgement.applied_at),
                ],
            );
        }
        #[cfg(feature = "acceptance")]
        if let Some(latency) = acceptance.acknowledge_mutation(
            acknowledgement.key,
            acknowledgement.token.generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
            mutation_cohort,
        ) {
            metrics.0.record_mutation_to_visible(latency);
        }
        stream.acknowledge_mesh_upload(
            acknowledgement.key,
            acknowledgement.token.generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
        );
    }
    let poll_report = std::mem::take(&mut frame_poll.report);
    stream.advance_block_cracks(if simulation_time.is_paused() {
        0.0
    } else {
        time.delta_secs()
    });
    ui_runtime.project_block_cracks(stream.block_crack_snapshot());
    let camera_position = view.eye_translation();
    let resolved_surface_spawn = client_world.pending_surface_spawn.and_then(|anchor| {
        client_world
            .stream
            .as_ref()
            .and_then(|stream| stream.surface_eye_position(anchor[0], anchor[1]))
    });
    #[cfg(feature = "acceptance")]
    let resolved_mutation_coordinate = acceptance.mutation_surface_anchor().and_then(|anchor| {
        client_world.stream.as_ref().and_then(|stream| {
            stream
                .surface_eye_position(anchor[0], anchor[1])
                .map(|position| deterministic_mutation_coordinate(position, anchor))
        })
    });

    let send_error = client_world.stream.as_mut().and_then(|stream| {
        let actor_position = local_physics
            .network_position()
            .unwrap_or(stream.resolved_server_position().position);
        let center = sub_chunk_request_center(
            stream,
            camera_publication.as_deref(),
            clock.session_generation(),
            actor_position,
        )?;
        flush_sub_chunk_requests(
            stream,
            OUTBOUND_SEND_BUDGET_PER_FRAME,
            center,
            |receipt, packet| network.send_sub_chunk_request(receipt, packet),
        )
        .err()
    });
    let mut published_items = 0_usize;
    let mut published_payload_items = 0_usize;
    let mut published_bytes = 0_u64;
    if let Some(stream) = client_world.stream.as_mut() {
        // Block changes from this frame's interaction or ingress publish once their workers
        // finish, not one poll later.
        stream.service_urgent_work();
        while let Some(change) = stream.pop_mesh_change() {
            if !mesh_change_has_publication_permit(&change) {
                let restored = stream.retry_mesh_change_front(change).is_ok();
                record_fatal_error(
                    &mut fatal_error.0,
                    if restored {
                        MISSING_PUBLICATION_PERMIT_ERROR.to_owned()
                    } else {
                        format!(
                            "{MISSING_PUBLICATION_PERMIT_ERROR}; failed to restore the rejected update to the bounded world retry FIFO"
                        )
                    },
                );
                break;
            }
            let change_bytes = match &change {
                WorldMeshChange::Upsert { mesh, biome, .. } => {
                    ChunkRenderQueue::upload_byte_len(mesh, biome)
                }
                WorldMeshChange::Remove { .. } => 0,
            };
            let retry = match change {
                WorldMeshChange::Upsert {
                    output_permit,
                    key,
                    mesh,
                    biome,
                    tint_identity,
                    generation,
                    dirty_since,
                    urgent,
                    permit,
                } => {
                    // The mesh moves into the render queue, so keep a copy of its diagnostic
                    // geometry only when publishing it would change the tracker.
                    let diagnostic_geometry = (!diagnostic_quads
                        .0
                        .is_current(key, mesh.diagnostic_geometry()))
                    .then(|| mesh.diagnostic_geometry().clone());
                    let publication_permit =
                        permit.expect("publication permit was validated before render handoff");
                    match render_queue.try_update_tracked_with_biome_identity_permitted(
                        key,
                        mesh,
                        biome,
                        tint_identity,
                        if urgent {
                            ChunkUploadPriority::urgent()
                        } else {
                            ChunkUploadPriority::from_camera(key, camera_position)
                        },
                        ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        publication_permit,
                    ) {
                        Ok(()) => {
                            block_use.prediction_frames.staged(frame.0, key, generation);
                            if let Some(diagnostic_geometry) = &diagnostic_geometry {
                                diagnostic_quads.0.upsert(key, diagnostic_geometry);
                            }
                            None
                        }
                        Err((mesh, biome, permit)) => Some(WorldMeshChange::Upsert {
                            output_permit,
                            key,
                            mesh,
                            biome,
                            tint_identity,
                            generation,
                            dirty_since,
                            urgent,
                            permit: Some(permit),
                        }),
                    }
                }
                WorldMeshChange::Remove {
                    key,
                    generation,
                    dirty_since,
                    urgent,
                    permit,
                } => {
                    let publication_permit =
                        permit.expect("publication permit was validated before render handoff");
                    match render_queue.try_remove_tracked_permitted(
                        key,
                        if urgent {
                            ChunkUploadPriority::urgent()
                        } else {
                            ChunkUploadPriority::from_camera(key, camera_position)
                        },
                        ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        stream.terrain_removal_kind(key),
                        publication_permit,
                    ) {
                        Ok(()) => {
                            diagnostic_quads.0.remove(key);
                            None
                        }
                        Err((key, permit)) => Some(WorldMeshChange::Remove {
                            key,
                            generation,
                            dirty_since,
                            urgent,
                            permit: Some(permit),
                        }),
                    }
                }
            };
            let Some(retry) = retry else {
                published_items = published_items.saturating_add(1);
                if change_bytes != 0 {
                    published_payload_items = published_payload_items.saturating_add(1);
                    published_bytes = published_bytes.saturating_add(change_bytes);
                }
                continue;
            };
            if stream.retry_mesh_change_front(retry).is_err() {
                fatal_error.0 = Some(
                    "failed to restore a render update to the bounded world retry FIFO".to_owned(),
                );
            }
            break;
        }
    }
    if let Some(stream) = client_world.stream.as_ref() {
        let (pending_mesh_jobs, in_flight_mesh_jobs) = stream.mesh_job_counts();
        let previous = publication.diagnostics().last_work;
        let allowance = publication.allowance();
        publication.finish_frame(PublicationFrameWork {
            mesh_jobs_dispatched: poll_report.mesh_jobs_dispatched,
            mesh_changes_published: published_items,
            mesh_payloads_published: published_payload_items,
            mesh_bytes_published: published_bytes,
            pending_mesh_jobs,
            in_flight_mesh_jobs,
            upload_queue_items: render_queue.retained_len(),
            upload_queue_bytes: render_queue.pending_bytes(),
            allowance_live_permits: allowance.live_permits(),
            allowance_live_payload_bytes: allowance.live_payload_bytes(),
            stream_pending_mesh_changes: stream.pending_mesh_change_count(),
            ..previous
        });
    }
    if let Some(error) = send_error {
        record_fatal_error(&mut fatal_error.0, error);
    }
    if let Some(position) = resolved_surface_spawn {
        view.reanchor_camera();
        view.set_eye_translation(Vec3::from_array(position));
        let tick = local_physics.state().map_or(0, |state| state.tick);
        // World publication runs after local physics. The next frame's delta
        // starts after this anchor, so it must remain eligible for simulation.
        movement.reanchor_surface_spawn(tick, position);
        local_physics.reanchor_network_position(position, tick, true);
        movement.enforce_local_physics_authority(&mut local_physics);
        client_world.pending_surface_spawn = None;
        info!(position = ?position, "resolved temporary Bedrock spawn from packed terrain");
    }
    #[cfg(feature = "acceptance")]
    if let Some(coordinate) = resolved_mutation_coordinate {
        acceptance.set_mutation_coordinate(coordinate);
    }
}
