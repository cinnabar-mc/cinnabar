//! Hands the local simulation clock to the world, whose piston arms move with it.

use bevy::prelude::{DetectChangesMut, Res, ResMut};
use client_interaction::movement::LocalPhysicsController;

use crate::runtime::world::ClientWorld;

/// Records the tick local physics has simulated, so arm actors that arrive later continue
/// from the progress their arm reached and moving arms revise their columns.
pub(crate) fn record_simulation_tick(
    physics: Res<LocalPhysicsController>,
    mut client_world: ResMut<ClientWorld>,
) {
    let Some(tick) = physics.state().map(|state| state.tick) else {
        return;
    };
    let Some(stream) = client_world.bypass_change_detection().stream.as_mut() else {
        return;
    };
    if let Err(error) = stream.set_simulation_tick(tick) {
        bevy::log::warn!(?error, "piston arm columns kept their collision revision");
    }
}
