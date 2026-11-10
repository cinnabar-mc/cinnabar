use crate::runtime::network::resource_packs::PackApplication;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use client_presentation::environment::{WeatherState, WorldClock, apply_environment_control};
use client_runtime::network::NetworkHandle;
#[cfg(feature = "acceptance")]
use {
    crate::runtime::movement_evidence::MovementEvidenceEmitter,
    acceptance::movement_evidence::MovementEvidenceEventKind,
};
#[cfg(feature = "acceptance")]
use {acceptance::model_witness::ModelWitnessFileSource, diagnostics::write_stdout_marker};
pub(crate) mod camera_environment;
pub(super) mod committed_ui;
mod control_apply;
mod dimension;
mod input_locks;
mod local_retention;
pub(crate) mod publication;
mod respawn;
pub(crate) use committed_ui::drain_committed_ui_before_authority;
use committed_ui::refresh_player_list_cache_for_controls;
pub(crate) use dimension::advance_dimension_transfer;
#[cfg(test)]
mod player_list_tests;
mod service;
mod sub_chunk_requests;
mod terrain_generation;
pub(crate) mod vehicle_look;
mod view;
pub(crate) use service::configure_world_service;
pub(crate) use sub_chunk_requests::flush_sub_chunk_requests;
pub(crate) use sub_chunk_requests::sub_chunk_request_center;
pub(crate) use terrain_generation::synchronize_terrain_generation;

#[cfg(feature = "acceptance")]
use acceptance::committed_control::{
    model_gallery_camera_committed_marker, refresh_mutation_anchor_from_committed_control,
};
pub(crate) use control_apply::{MovePlayerRotation, apply_committed_aim, apply_committed_control};

use std::sync::Arc;

use assets::{RuntimeAssets, RuntimeEntityAssets};
use bevy::{
    ecs::system::SystemParam,
    log::info,
    prelude::{Local, MessageWriter, Res, ResMut, Resource, Time, Vec3},
    time::Real,
};
use chunk_pipeline::{CohortProgress, ViewCohortStatus, WorldStream, WorldStreamPoll};
use client_world::CommittedControlEvent;

use client_audio::audio_ingress::{SequencedAudioEvent, drain_committed_audio};
use client_presentation::server_camera::{ServerCameraInstructions, drain_committed_camera};
use protocol::BlobCacheStats;
use render_scene::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkUploadBudget, RuntimeStage, RuntimeStageProfiler,
    VisibilityDiagnosticsInput,
};

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::shutdown::{FatalError, record_fatal_error},
    client_interaction::movement::{
        LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
        MovementTicker, PhysicsCollisionRegistries,
    },
    client_presentation::{
        camera::CameraSettingsAuthority,
        local_player::{
            InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalPlayerFrameReset,
            LocalViewPose,
        },
    },
};

#[cfg(feature = "acceptance")]
fn position_distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let delta = Vec3::from_array(to) - Vec3::from_array(from);
    delta.length()
}

#[derive(Resource, Debug, Default)]
pub(crate) struct WorldStreamFramePoll {
    pub(crate) report: WorldStreamPoll,
    /// Committed-view readiness while startup or diagnostics watch it.
    pub(crate) cohort_progress: Option<CohortProgress>,
    /// The full committed-view witness, only while acceptance or metrics consume it.
    pub(crate) cohort: Option<ViewCohortStatus>,
}

/// A latched, bounded server-directed transfer target awaiting the launcher's
/// replacement handoff. `None` once consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferNotice {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Resource)]
pub(crate) struct ClientWorld {
    pub(crate) stream: Option<WorldStream>,
    pub(crate) movement_colliders: crate::movement::MovementColliderCache,
    pub(crate) block_rays: crate::block_ray_cache::BlockRayCache,
    pub(crate) runtime_assets: Arc<RuntimeAssets>,
    pub(crate) entity_assets: Option<Arc<RuntimeEntityAssets>>,
    /// The session's server-pack entities, layered over `entity_assets`.
    pub(crate) pack_entities: Option<Arc<assets::SessionEntityPack>>,
    /// Worker-computed terrain inputs used to select the far-terrain cache namespace.
    pub(crate) terrain_resource_identity: [u8; 32],
    /// Worker-built pack pages, reused only while their base artwork and pack remain current.
    pub(crate) prepared_actor_artwork:
        Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>,
    /// The session's custom item facts and pack icons for held and worn items.
    pub(crate) session_items: Option<Arc<client_presentation::session_assets::SessionItems>>,
    /// Item tag programs compiled with this session's block registries.
    pub(crate) mining_filters: Arc<pack_runtime::CustomMiningFilters>,
    /// Block tag programs compiled from the session item registry's digger components.
    pub(crate) digger_filters: Arc<pack_runtime::DiggerFilters>,
    /// One registry held at its ingress position until its digger queries are ready.
    pub(crate) pending_digger_registry: Option<super::network::digger_registry::PendingRegistry>,
    pub(crate) pending_surface_spawn: Option<[i32; 2]>,
    pub(crate) dimension_transfer: dimension::DimensionTransfer,
    pub(crate) respawn: respawn::RespawnLifecycle,
    pub(crate) transfer_notice: Option<TransferNotice>,
    pub(crate) network_decode_errors: u64,
    pub(crate) client_blob_cache_enabled: bool,
    pub(crate) client_blob_cache: BlobCacheStats,
    /// StartGame's hashed world identity, absent when no level ID was supplied.
    pub(crate) level_identity: Option<u64>,
}

impl Default for ClientWorld {
    fn default() -> Self {
        Self::new(Arc::new(RuntimeAssets::diagnostic()))
    }
}

impl ClientWorld {
    pub(crate) fn new(runtime_assets: Arc<RuntimeAssets>) -> Self {
        Self {
            stream: None,
            movement_colliders: Default::default(),
            block_rays: Default::default(),
            runtime_assets,
            entity_assets: None,
            pack_entities: None,
            terrain_resource_identity: [0; 32],
            prepared_actor_artwork: None,
            session_items: None,
            mining_filters: Arc::default(),
            digger_filters: Arc::default(),
            pending_digger_registry: None,
            pending_surface_spawn: None,
            dimension_transfer: dimension::DimensionTransfer::default(),
            respawn: respawn::RespawnLifecycle::default(),
            transfer_notice: None,
            network_decode_errors: 0,
            client_blob_cache_enabled: false,
            client_blob_cache: BlobCacheStats::default(),
            level_identity: None,
        }
    }

    pub(crate) fn new_with_entity_assets(
        runtime_assets: Arc<RuntimeAssets>,
        entity_assets: Arc<RuntimeEntityAssets>,
    ) -> Self {
        Self {
            entity_assets: Some(entity_assets),
            ..Self::new(runtime_assets)
        }
    }

    /// Unmapped block lookups on the assets the current session meshes with,
    /// which carry any server block overlay.
    pub(crate) fn missing_asset_count(&self) -> u64 {
        self.stream
            .as_ref()
            .map_or(&self.runtime_assets, |stream| stream.runtime_assets())
            .missing_count()
    }
}

#[derive(SystemParam)]
pub(crate) struct AppWorldState<'w> {
    pub(crate) client_world: ResMut<'w, ClientWorld>,
    pub(crate) fatal_error: ResMut<'w, FatalError>,
    pub(crate) clock: ResMut<'w, WorldClock>,
    pub(crate) weather: ResMut<'w, WeatherState>,
    pub(crate) movement: ResMut<'w, MovementTicker>,
    pub(crate) local_physics: ResMut<'w, LocalPhysicsController>,
    pub(crate) movement_effects: ResMut<'w, LocalMovementEffectTimeline>,
    pub(crate) movement_speed: ResMut<'w, LocalMovementSpeedAuthority>,
    pub(crate) collisions: ResMut<'w, PhysicsCollisionRegistries>,
    pub(crate) ui_runtime: ResMut<'w, UiRuntimeResource>,
    pub(crate) time: Res<'w, Time<Real>>,
}

pub(crate) fn startup_biome_tints(runtime_assets: &RuntimeAssets) -> ChunkBiomeTints {
    let resolved = runtime_assets
        .biome_assets()
        .resolve_live(&[])
        .expect("validated startup biome assets resolve without live definitions");
    ChunkBiomeTints::from_resolved(&resolved, 0)
}

pub(crate) fn synchronize_biome_tints(stream: &WorldStream, active: &mut ChunkBiomeTints) -> bool {
    let identity = stream.biome_tint_identity();
    if active.table_identity() == identity {
        return false;
    }
    let resolved = stream.resolved_biome_tints_snapshot();
    *active = ChunkBiomeTints::from_resolved_with_identity(&resolved, identity);
    true
}

/// Computes startup readiness from required columns and full diagnostics only when enabled.
/// Once startup releases, ordinary play scans neither.
pub(crate) fn frame_cohort_status(
    stream: &WorldStream,
    #[cfg(feature = "acceptance")] acceptance: &AcceptanceRun,
    startup_probe_enabled: bool,
) -> (Option<CohortProgress>, Option<ViewCohortStatus>) {
    let diagnostics_enabled = {
        #[cfg(feature = "acceptance")]
        {
            super::telemetry::publication_diagnostics_enabled(acceptance)
        }
        #[cfg(not(feature = "acceptance"))]
        {
            false
        }
    };
    let Some(target) = stream.committed_view_cohort() else {
        return (None, None);
    };
    if diagnostics_enabled {
        let status = stream.cohort_status(target);
        (Some(status.into()), Some(status))
    } else if startup_probe_enabled {
        (Some(stream.cohort_progress(target)), None)
    } else {
        (None, None)
    }
}

pub(crate) fn world_stream_fatal_message(error: chunk_pipeline::WorldStreamFatalError) -> String {
    format!("world stream fatal: {error}")
}

/// Diagnostics, player authority and per-system correction history used while polling the world.
#[derive(SystemParam)]
pub(crate) struct WorldPollInputs<'w, 's> {
    visibility_diagnostics: Option<Res<'w, VisibilityDiagnosticsInput>>,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    player_runtime: ResMut<'w, client_state::PlayerRuntime>,
    server_camera_view: Option<Res<'w, client_presentation::camera::ServerCameraView>>,
    correction_log: Local<'s, gameplay::movement::CorrectionContextLog>,
    item_use: Option<ResMut<'w, client_interaction::item_use::ItemUseRuntime>>,
    locomotion: ResMut<'w, client_interaction::movement::LocomotionState>,
    look_smoother: ResMut<'w, client_presentation::camera::look::LookSmoother>,
    #[cfg(not(target_arch = "wasm32"))]
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    render_queue: Option<ResMut<'w, ChunkRenderQueue>>,
    #[cfg(not(target_arch = "wasm32"))]
    mods: Option<ResMut<'w, crate::modding::ModRuntime>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_world_stream_before_physics(
    state: AppWorldState,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    #[cfg(feature = "acceptance")] mut acceptance: ResMut<AcceptanceRun>,
    upload_budget: Res<ChunkUploadBudget>,
    #[cfg(feature = "acceptance")] model_witness_source: Res<ModelWitnessFileSource>,
    mut camera_settings: ResMut<CameraSettingsAuthority>,
    mut view: ResMut<LocalViewPose>,
    mut local_frame: ResMut<LocalPlayerFrameCarrier>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    #[cfg(feature = "acceptance")] mut movement_evidence: ResMut<MovementEvidenceEmitter>,
    mut frame_poll: ResMut<WorldStreamFramePoll>,
    mut audio: MessageWriter<SequencedAudioEvent>,
    mut server_camera: ResMut<ServerCameraInstructions>,
    mut camera_hurt: Option<ResMut<client_presentation::camera::CameraHurtState>>,
    mut particle_inbox: Option<ResMut<crate::particles::ParticleInbox>>,
    inputs: WorldPollInputs<'_, '_>,
) {
    let WorldPollInputs {
        visibility_diagnostics,
        profiler,
        mut player_runtime,
        server_camera_view,
        mut correction_log,
        mut item_use,
        mut locomotion,
        mut look_smoother,
        #[cfg(not(target_arch = "wasm32"))]
        menu,
        #[cfg(target_arch = "wasm32")]
        options,
        mut render_queue,
        #[cfg(not(target_arch = "wasm32"))]
        mut mods,
    } = inputs;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::WorldPoll));
    let AppWorldState {
        mut client_world,
        mut fatal_error,
        mut clock,
        mut weather,
        mut movement,
        mut local_physics,
        mut movement_effects,
        mut movement_speed,
        mut ui_runtime,
        collisions,
        time,
        ..
    } = state;
    let ClientWorld {
        stream,
        movement_colliders,
        pending_surface_spawn,
        dimension_transfer,
        respawn,
        ..
    } = &mut *client_world;
    let Some(stream) = stream.as_mut() else {
        *frame_poll = WorldStreamFramePoll::default();
        local_frame.reset(LocalPlayerFrameReset::Session);
        interaction.invalidate();
        return;
    };
    stream.set_view_forward(view::streaming_forward(&view, &camera_settings));
    #[cfg(not(target_arch = "wasm32"))]
    let options = menu.as_ref().map(|menu| menu.settings_snapshot().0);
    #[cfg(target_arch = "wasm32")]
    let options = Some(Arc::clone(&options.0));
    let rendering = options
        .as_ref()
        .map_or_else(meshing::MeshRenderSettings::default, |options| {
            use launcher::menu::settings_options::SettingId;
            meshing::MeshRenderSettings {
                smooth_lighting: options.value(SettingId::SmoothLighting) != 0,
                transparent_leaves: options.value(SettingId::TransparentLeaves) != 0,
            }
        });
    if stream.set_mesh_render_settings(rendering, web_time::Instant::now())
        && let Some(queue) = render_queue.as_mut()
    {
        queue.discard_resource_work();
    }
    // Adds to the between-frames service's work already recorded for this frame.
    frame_poll.report.accumulate(stream.poll(
        view.eye_translation().to_array(),
        upload_budget.max_per_frame,
    ));
    (frame_poll.cohort_progress, frame_poll.cohort) = frame_cohort_status(
        stream,
        #[cfg(feature = "acceptance")]
        &acceptance,
        visibility_diagnostics.is_some_and(|diagnostics| diagnostics.startup_probe_enabled()),
    );
    drain_committed_audio(stream, |event| {
        audio.write(event);
    });
    if let Some(inbox) = particle_inbox.as_mut() {
        crate::particles::drain_committed_particles(stream, inbox);
    }
    drain_committed_camera(
        stream,
        clock.session_generation(),
        stream.current_dimension(),
        &mut server_camera,
    );
    if let Some(error) = stream.take_fatal_error() {
        movement.deactivate();
        local_physics.deactivate();
        local_frame.reset(LocalPlayerFrameReset::Session);
        interaction.invalidate();
        record_fatal_error(&mut fatal_error.0, world_stream_fatal_message(error));
        return;
    }

    respawn.synchronize_session(Some(clock.session_generation()));
    let controls = stream.take_committed_controls();
    respawn.retain_committed_input_hold(&controls);
    if network
        .as_ref()
        .is_some_and(|network| network.flush_latency_reply().is_err())
    {
        movement.set_control_fence_pending(true);
        stream.restore_committed_controls(controls.into_iter());
        return;
    }
    movement.set_control_fence_pending(false);
    refresh_player_list_cache_for_controls(stream, &mut ui_runtime, &controls);

    let mut controls = controls.into_iter();
    while let Some(control) = controls.next() {
        if let Err(error) = respawn.queue_completion(|packet| match network.as_ref() {
            Some(network) => network.send_dimension_packet(packet),
            None => Err(client_session::PacketSendError::Full(packet)),
        }) {
            if error.is_closed() {
                record_fatal_error(&mut fatal_error.0, format!("respawn completion: {error}"));
            }
            movement.set_control_fence_pending(true);
            stream.restore_committed_controls(std::iter::once(control).chain(controls));
            return;
        }
        if let CommittedControlEvent::ClientInputLocks { locks, .. } = control {
            input_locks::apply_committed_input_locks(
                clock.session_generation(),
                locks,
                &mut player_runtime.facts,
                &mut local_physics,
                &mut locomotion,
                &mut look_smoother,
            );
            continue;
        }
        if let CommittedControlEvent::DimensionChangeAck {
            dimension_epoch, ..
        } = control
        {
            dimension_transfer.acknowledge(dimension_epoch);
            continue;
        }
        if let CommittedControlEvent::NetworkStackLatency { creation_time, .. } = control {
            // Earlier controls already applied; the reply and later ones wait for older inputs to send.
            if movement.has_unsent_inputs() {
                movement.set_control_fence_pending(true);
                stream.restore_committed_controls(std::iter::once(control).chain(controls));
                return;
            }
            let Some(network) = network.as_ref() else {
                movement.set_control_fence_pending(true);
                stream.restore_committed_controls(std::iter::once(control).chain(controls));
                return;
            };
            match network.send_latency_reply(creation_time) {
                Ok(()) => {
                    movement.set_control_fence_pending(network.has_pending_latency_reply());
                    gameplay::movement::trace_server_control(&movement, &local_physics, &control)
                }
                Err(client_session::BatchSendError::Full) => {
                    movement.set_control_fence_pending(true);
                    stream.restore_committed_controls(std::iter::once(control).chain(controls));
                    return;
                }
                Err(client_session::BatchSendError::Closed) => return,
            }
            continue;
        }
        gameplay::movement::trace_server_control(&movement, &local_physics, &control);
        if let Some(packet) = gameplay::movement::CorrectionPacket::from_control(&control) {
            let camera = server_camera_view.as_deref();
            correction_log.note(
                movement.session_generation(),
                web_time::Instant::now(),
                packet,
                gameplay::movement::CorrectionContext::read(
                    &player_runtime.facts,
                    movement.physics_is_authorized(),
                    camera.and_then(|camera| camera.active_preset_name()),
                    camera.is_some_and(|camera| camera.has_pose_override()),
                ),
            );
        }
        if let CommittedControlEvent::LocalMovementFlags {
            tick,
            flags,
            replacement,
            ..
        } = control
        {
            if replacement {
                player_runtime
                    .facts
                    .clear_actor_movement_authority(clock.session_generation());
            }
            let previous = player_runtime.facts.is_immobile();
            player_runtime
                .facts
                .apply_local_movement_flags(clock.session_generation(), flags);
            if previous != player_runtime.facts.is_immobile() {
                info!(
                    immobile = player_runtime.facts.is_immobile(),
                    tick, "server changed local player immobility"
                );
            }
        }
        if let CommittedControlEvent::LocalMovementSpeed {
            air_drag_modifier: Some(current),
            ..
        } = control
        {
            player_runtime
                .facts
                .apply_air_drag_modifier(clock.session_generation(), current);
        }
        if respawn.consume_nonspatial_phase(
            clock.session_generation(),
            &control,
            stream.local_player_runtime_id(),
            &mut movement,
            &mut local_physics,
            item_use.as_deref_mut(),
        ) {
            continue;
        }
        if let CommittedControlEvent::ChangeDimension {
            sequence,
            change,
            resolved,
        } = control
        {
            dimension_transfer.begin(
                clock.session_generation(),
                sequence,
                protocol::ChangeDimensionEvent {
                    position: resolved.position,
                    ..change
                },
                stream.local_player_runtime_id(),
                !respawn.awaiting_respawn(),
                time.elapsed(),
            );
        }
        let world = crate::movement::collision_world(
            stream,
            collisions.registry(stream.network_id_mode()),
            movement_colliders,
        );
        let disposition = gameplay::committed_control::CommittedGameplayState {
            movement: &mut movement,
            physics: &mut local_physics,
            effects: &mut movement_effects,
            speed: &mut movement_speed,
            session_generation: clock.session_generation(),
            dimension: stream.current_dimension(),
            dimension_transfer_active: dimension_transfer.active(),
        }
        .apply(control, &world, |observation| {
            use gameplay::committed_control::ControlObservation;
            match observation {
                ControlObservation::Hurt { source_direction } => {
                    if let Some(hurt) = camera_hurt.as_deref_mut() {
                        hurt.register(client_presentation::camera::LocalHurtEvent {
                            source_direction,
                            ..Default::default()
                        });
                    }
                }
                ControlObservation::Knockback { motion } => {
                    if let Some(hurt) = camera_hurt.as_deref_mut() {
                        hurt.note_knockback(motion[0], motion[2]);
                    }
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::BeforeSpatial(control) => {
                    let _ =
                        refresh_mutation_anchor_from_committed_control(&mut acceptance, &control);
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::Correction {
                    outcome,
                    previous,
                    position,
                } => {
                    movement_evidence
                        .note_correction(outcome, position_distance(previous, position));
                }
                #[cfg(feature = "acceptance")]
                ControlObservation::Dimension => {
                    movement_evidence.note_event(MovementEvidenceEventKind::Dimension);
                }
                #[cfg(not(feature = "acceptance"))]
                ControlObservation::BeforeSpatial(_)
                | ControlObservation::Correction { .. }
                | ControlObservation::Dimension => {}
            }
        });
        use gameplay::committed_control::{ControlDisposition, SpatialReset};
        let mut rotation_filter = control_apply::rotation_filter(
            #[cfg(not(target_arch = "wasm32"))]
            mods.as_deref_mut(),
        );
        let reset = match disposition {
            ControlDisposition::Handled => continue,
            ControlDisposition::Aim => {
                apply_committed_aim(control, &view, &mut local_physics, &mut rotation_filter);
                continue;
            }
            ControlDisposition::Environment => {
                apply_environment_control(
                    control,
                    &mut clock,
                    &mut weather,
                    time.elapsed_secs_f64(),
                );
                continue;
            }
            ControlDisposition::Spatial(SpatialReset::Correction) => {
                LocalPlayerFrameReset::Correction
            }
            ControlDisposition::Spatial(SpatialReset::Dimension) => {
                LocalPlayerFrameReset::Dimension
            }
        };
        local_frame.reset(reset);
        interaction.invalidate();
        #[cfg(feature = "acceptance")]
        let _ = acceptance.observe_committed_full_view_control(&control);
        #[cfg(feature = "acceptance")]
        let camera_marker =
            model_gallery_camera_committed_marker(model_witness_source.configured(), &control);
        apply_committed_control(
            control,
            &mut view,
            &mut camera_settings,
            pending_surface_spawn,
            &mut rotation_filter,
        );
        #[cfg(feature = "acceptance")]
        if let Some(marker) = camera_marker {
            let mut stdout = diagnostics::console::stdout();
            write_stdout_marker(&mut stdout, &marker);
        }
    }
}
