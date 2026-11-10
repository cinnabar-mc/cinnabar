use super::WorldAuthority;
use protocol::{ActorMoveEvent, ActorPositionOrigin};
use world::ChunkKey;

/// Forced actor moves snap when their displacement reaches half a block.
const ACTOR_TELEPORT_DISTANCE_SQUARED: f32 = 0.25;

impl WorldAuthority {
    /// Checks the Normal MovePlayer target before the actor store chooses direct placement or lerp.
    pub(super) fn player_move_target_resident(&self, event: &protocol::ActorEvent) -> bool {
        let protocol::ActorEvent::Move(movement) = event else {
            return true;
        };
        if movement.player_mode != Some(protocol::MovePlayerMode::Normal) {
            return true;
        }
        let Some(actor) = self.actors.get(movement.runtime_id) else {
            return true;
        };
        let current = actor.last_received_native_position();
        let coordinate = |axis: usize| {
            movement.position[axis]
                .filter(|value| value.is_finite())
                .unwrap_or(current[axis])
                .floor() as i32
        };
        self.terrain.contains_column(ChunkKey::new(
            movement.dimension,
            coordinate(0).div_euclid(16),
            coordinate(2).div_euclid(16),
        ))
    }

    /// Resolves MoveActor snapping against its decoded target and currently resident terrain.
    pub(super) fn resolve_actor_teleport(&self, movement: &mut ActorMoveEvent) {
        if movement.player_mode.is_some()
            || movement.position_origin != ActorPositionOrigin::NetworkOffset
            || movement.runtime_id == self.local_player_runtime_id
            || movement.dimension != self.current_dimension
        {
            return;
        }
        let Some(actor) = self.actors.get(movement.runtime_id) else {
            return;
        };
        let mut target = actor.last_received_native_position();
        for (coordinate, update) in target.iter_mut().zip(movement.position) {
            if let Some(value) = update.filter(|value| value.is_finite()) {
                *coordinate = value;
            }
        }
        let current = actor.native_position();
        let delta: [f32; 3] = std::array::from_fn(|axis| target[axis] - current[axis]);
        let distance_squared = delta[2] * delta[2] + delta[1] * delta[1] + delta[0] * delta[0];
        let column = ChunkKey::new(
            movement.dimension,
            (target[0].floor() as i32).div_euclid(16),
            (target[2].floor() as i32).div_euclid(16),
        );
        movement.teleported = (movement.teleported
            && distance_squared >= ACTOR_TELEPORT_DISTANCE_SQUARED)
            || !self.terrain.contains_column(column);
    }
}

#[cfg(test)]
#[path = "actor_movement_tests.rs"]
mod tests;
