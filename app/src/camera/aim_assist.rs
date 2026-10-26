//! Packet-driven aim selection publishes a separate interaction ray.

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use std::sync::Arc;

use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::{
    aim_assist::{AimAssistFrame, ServerAimAssist},
    server_camera::ServerCameraInstructions,
};
use protocol::CameraAimAssistPresetSettings;
use semantic_input::PerspectiveMode;

use {
    crate::runtime::world::ClientWorld,
    client_interaction::movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    client_presentation::environment::WorldClock,
    client_presentation::{
        camera::{CameraSettingsAuthority, ServerCameraView},
        local_player::InteractionOriginSnapshot,
    },
    client_state::PlayerRuntime,
};

/// Retains only activation identity; steady frames create no outbound packets.
#[derive(Default)]
pub(crate) struct AimActivation {
    identity: Option<(u64, i32)>,
    camera: Option<Arc<str>>,
    settings: Option<CameraAimAssistPresetSettings>,
    supported: Option<bool>,
    perspective: Option<PerspectiveMode>,
}

/// The published ray and server policies share one ordered frame boundary.
#[derive(SystemParam)]
pub(crate) struct AimContext<'w> {
    world: Res<'w, ClientWorld>,
    clock: Res<'w, WorldClock>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    player: Res<'w, PlayerRuntime>,
    instructions: Res<'w, ServerCameraInstructions>,
    camera: Res<'w, ServerCameraView>,
    camera_settings: Res<'w, CameraSettingsAuthority>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    physics: Res<'w, LocalPhysicsController>,
}

/// Native assistance changes interaction aim on every device without adding continuous look input.
pub(crate) fn publish_assisted_interaction(
    context: AimContext,
    mut state: ResMut<ServerAimAssist>,
    mut frame: ResMut<AimAssistFrame>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    mut activation: Local<AimActivation>,
    mut movement: ResMut<client_interaction::movement::MovementTicker>,
) {
    let identity = context.world.stream.as_ref().map(|stream| {
        (
            context.clock.session_generation(),
            stream.current_dimension(),
        )
    });
    state.observe_identity(identity);
    let previous_skips = state.semantic_skips();
    for instruction in context.instructions.iter() {
        state.apply(instruction.sequence, &instruction.event);
    }
    frame.synchronize(&state);
    if state.semantic_skips() != previous_skips {
        warn!(
            skipped = state.semantic_skips() - previous_skips,
            "skipped unknown aim-assist presets"
        );
    }
    let Some(stream) = context.world.stream.as_ref() else {
        return;
    };
    activation.publish(
        &context.camera,
        context.camera_settings.perspective(),
        &state,
        identity,
        |packet| {
            context
                .network
                .send_settings_packet(context.clock.session_generation(), packet)
                .is_ok()
        },
    );
    let Some(ray) = interaction.outbound_ray() else {
        return;
    };
    let tick = ray.physics_tick().get();
    let mode = stream.network_id_mode();
    let world = aim_world(stream, &context.collisions, &context.player);
    let item = context
        .player
        .selected_stack()
        .filter(|stack| !stack.is_empty())
        .and_then(|stack| stream.authority().item_identifier(stack.network_id));
    let previous_queries = frame.skipped_queries;
    for (tick, eye, forward) in context.physics.aim_poses_after(frame.last_tick(), tick) {
        frame.evaluate(
            &state,
            stream.authority(),
            &world,
            tick,
            Vec3::from_array(eye),
            Vec3::from_array(forward),
            stream.authority().actor(stream.local_player_runtime_id()),
            item.as_ref(),
            |id| context.collisions.block_identifier(mode, id),
            |id| context.collisions.block_tags(mode, id),
        );
        if state.settings().is_some() {
            movement.publish_interaction_aim(
                tick,
                frame
                    .interaction_direction()
                    .map(|direction| direction.to_array()),
            );
        }
    }
    if frame.skipped_queries != previous_queries
        && (previous_queries == 0 || previous_queries / 64 != frame.skipped_queries / 64)
    {
        warn!(
            total = frame.skipped_queries,
            "skipped unavailable aim-assist target data"
        );
    }
    if let Some(direction) = frame.interaction_direction() {
        interaction.assist_direction(direction);
    }
}

impl AimActivation {
    /// Camera activation requests policy from the server, which owns the resulting settings.
    fn publish(
        &mut self,
        view: &ServerCameraView,
        perspective: PerspectiveMode,
        state: &ServerAimAssist,
        identity: Option<(u64, i32)>,
        send: impl FnOnce(protocol::Packet) -> bool,
    ) {
        let camera = view.active_preset_name();
        let settings = view.active_aim_assist();
        let supported = view.active_base_preset_name().map_or_else(
            || perspective != PerspectiveMode::FirstPerson,
            |base| base != "minecraft:first_person",
        );
        let perspective = camera.is_none().then_some(perspective);
        if self.identity == identity
            && self.camera.as_deref() == camera
            && self.settings.as_ref() == settings
            && self.supported == Some(supported)
            && self.perspective == perspective
        {
            return;
        }
        let clear = !supported || settings.is_none();
        if !clear
            && !state.has_preset(
                settings
                    .and_then(|settings| settings.preset_id.as_deref())
                    .unwrap_or(client_presentation::aim_assist::DEFAULT_AIM_ASSIST_PRESET),
            )
        {
            return;
        }
        let packet = protocol::camera_aim_assist_activation_packet(
            if clear { "" } else { camera.unwrap_or("") },
            supported,
            clear,
        );
        if send(packet) {
            self.identity = identity;
            self.camera = camera.map(Arc::from);
            self.settings = settings.cloned();
            self.supported = Some(supported);
            self.perspective = perspective;
        }
    }
}

/// Supplies current player eligibility to native aim candidate and obstruction sampling.
fn aim_world<'a>(
    stream: &'a chunk_pipeline::WorldStream,
    collisions: &'a PhysicsCollisionRegistries,
    player: &PlayerRuntime,
) -> sim::PaletteWorld<'a> {
    sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
        crate::interaction_authority::pick_context(
            stream,
            collisions,
            player.facts.player_game_mode(),
            player.selected_stack().map(|item| item.block_runtime_id),
        ),
    )
}

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod pick_tests;
