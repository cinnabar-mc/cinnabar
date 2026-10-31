//! Block use as standalone click-block transactions on the press and while held.
//!
//! The local use outcome (interaction, placement or nothing) decides the
//! transaction's prediction and swing. A validated placement or switch toggle
//! is also applied locally; the server's block updates stay authoritative. Air
//! use lives in `item_use`.

use client_interaction::block_use::{
    BlockUseRuntime, FramePick, held_block_store_id, predicted_placement, predicted_toggle,
    verified_use_selection,
};

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Time, Window, With},
    window::PrimaryWindow,
};
use client_runtime::network::NetworkHandle;
use protocol::{ItemUseTrigger, PlayerGameMode, PlayerInputMode};
use semantic_input::Action;
use sim::PaletteWorld;

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::world::ClientWorld,
    client_interaction::{
        melee::{MeleeRuntime, SwingTracker},
        movement::{LocalMovementEffectTimeline, MovementTicker, PhysicsCollisionRegistries},
    },
    client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose},
    client_state::SemanticInputSnapshot,
    gameplay::{
        interaction_authority::FrozenBlockObservation, melee::obstructs_placement,
        mining::protocol_input_mode,
    },
};

mod actor_use;
mod build;
mod world_observation;
use world_observation::use_surroundings;
pub(crate) mod ordered;

use gameplay::block_use::{LocalUse, RepeatClock, UseSurroundings, placement_cell, use_packets};

#[derive(SystemParam)]
pub(crate) struct BlockUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    ordered: Option<Res<'w, crate::ordered_interactions::InteractionRuntime>>,
    frame: Res<'w, bevy::diagnostic::FrameCount>,
    origin: Res<'w, InteractionOriginSnapshot>,
    view: Res<'w, LocalViewPose>,
    ui: Res<'w, UiRuntimeResource>,
    focus: Res<'w, client_state::ScreenFocus>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: ResMut<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    melee: Res<'w, MeleeRuntime>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    time: Res<'w, Time<Real>>,
    /// The level's sneaking dimensions size the player box that blocks placement.
    physics: Option<Res<'w, client_interaction::movement::LocalPhysicsController>>,
    audio_cues: bevy::prelude::MessageWriter<'w, client_audio::audio::LocalBlockCue>,
}

/// Drains physical interactions before physics, then evaluates the buffered held picks.
pub(crate) fn produce_block_use(
    world: &mut bevy::prelude::World,
    mut windows: bevy::prelude::Local<crate::ordered_interactions::PrimaryWindows>,
) {
    if !world.contains_resource::<PhysicsCollisionRegistries>() {
        return;
    }
    crate::ordered_interactions::drain(
        world,
        crate::ordered_interactions::DispatchPhase::BeforeMovement,
        &mut windows,
    );
    if !crate::ordered_interactions::pending(world) {
        crate::ordered_interactions::run_cached(world, maintain_block_use, ())
            .expect("block maintenance resources");
    } else {
        crate::ordered_interactions::run_cached(world, held::discard_tick_picks, ())
            .expect("block pick retirement resources");
    }
}

mod held;
use held::maintain_block_use;

mod pick;
pub(crate) use pick::retain_block_use_pick;

mod target;
use target::observe_use_target;

/// Sends the held-use stop action before dropping its successful-placement history.
fn stop_block_use(runtime: &mut BlockUseRuntime, context: &BlockUseContext, repress: bool) -> bool {
    let Some(stream) = context.client_world.stream.as_ref() else {
        runtime.clear();
        return true;
    };
    let packets = runtime.stop_packets(stream.local_player_runtime_id(), repress);
    let admitted = packets.is_empty() || context.network.send_inventory_packets(packets).is_ok();
    runtime.admit_stop(admitted)
}

#[cfg(test)]
#[path = "block_use/published_tests.rs"]
mod published_tests;

#[cfg(test)]
mod startup_tests {
    #[test]
    fn input_maintenance_waits_for_deferred_world_resources() {
        let mut world = bevy::prelude::World::new();
        world.run_system_cached(super::produce_block_use).unwrap();
        assert!(!world.contains_resource::<crate::ordered_interactions::InteractionRuntime>());
    }
}
