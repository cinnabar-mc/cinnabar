//! Admission of remote movement and its retained interpolation targets.

use super::*;

impl ActorStore {
    /// Applies one admitted movement using terrain residency at this packet boundary.
    pub(super) fn apply_movement(
        &mut self,
        sequence: u64,
        mut movement: protocol::ActorMoveEvent,
        player_target_resident: bool,
    ) -> ActorApplyResult {
        movement.teleported |= movement.player_mode == Some(MovePlayerMode::Reset);
        // The local player's pose is client-fed each tick; server movement
        // (authoritative reconciliation) must not fight that feed.
        if self.remote_state_excluded_runtime_id == Some(movement.runtime_id) {
            return ActorApplyResult::MissingActor;
        }
        if !self.admit_local_vehicle_move(&movement) {
            return ActorApplyResult::Updated;
        }
        let Some(actor) = self.actors.get_mut(&movement.runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        if movement.player_mode == Some(MovePlayerMode::Rotation) {
            actor.apply_player_rotation(&movement, sequence);
            return ActorApplyResult::Updated;
        }
        let Some(duration) = super::movement_interpolation::duration(movement.interpolation) else {
            let previous = self.ignored_movement_components;
            self.ignored_movement_components = previous.saturating_add(1);
            if previous == 0 || self.ignored_movement_components / 64 > previous / 64 {
                tracing::warn!(
                    "ignored unsupported actor movement duration (total {})",
                    self.ignored_movement_components
                );
            }
            return ActorApplyResult::Updated;
        };
        let previous_received = actor.last_received_pose();
        let mut received = previous_received;
        let network_position_offset =
            if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                actor.network_position_offset()
            } else {
                0.0
            };
        let mut ignored = 0_u64;
        let mut merge = |target: &mut f32, source: Option<f32>| {
            if let Some(value) = source {
                if value.is_finite() {
                    *target = value;
                } else {
                    ignored += 1;
                }
            }
        };
        for (axis, (target, source)) in received
            .position
            .iter_mut()
            .zip(movement.position)
            .enumerate()
        {
            merge(
                target,
                source.map(|value| {
                    if axis == 1 {
                        value - network_position_offset
                    } else {
                        value
                    }
                }),
            );
        }
        if movement.position[1].is_some_and(f32::is_finite) {
            received.y_origin = if movement.position_origin == ActorPositionOrigin::NetworkOffset {
                PoseYOrigin {
                    native_y: movement.position[1],
                    ..PoseYOrigin::NETWORK
                }
            } else {
                PoseYOrigin::default()
            };
        }
        merge(&mut received.pitch, movement.pitch);
        merge(&mut received.yaw, movement.yaw);
        merge(&mut received.head_yaw, movement.head_yaw);
        if ignored > 0 {
            let previous = self.ignored_movement_components;
            self.ignored_movement_components = previous.saturating_add(ignored);
            if previous == 0 || self.ignored_movement_components / 64 > previous / 64 {
                tracing::warn!(
                    "ignored non-finite actor movement components: {}",
                    self.ignored_movement_components
                );
            }
        }
        if let Some(value) = movement.on_ground {
            actor.on_ground = Some(value);
        }
        let tick_seconds = simulation_clock::TICK_DURATION.as_secs_f32();
        let elapsed_seconds = movement
            .source_tick
            .zip(actor.source_tick)
            .and_then(|(current, previous)| current.checked_sub(previous))
            .filter(|ticks| *ticks > 0)
            .map_or(tick_seconds, |ticks| ticks as f32 * tick_seconds);
        let derived_velocity = if movement.teleported {
            [0.0; 3]
        } else {
            std::array::from_fn(|axis| {
                (received.position[axis] - previous_received.position[axis]) / elapsed_seconds
            })
        };
        // A rotation-only move carries no displacement to derive from.
        if movement.position.iter().any(Option::is_some) {
            actor.velocity = if derived_velocity.iter().all(|value| value.is_finite()) {
                derived_velocity
            } else {
                [0.0; 3]
            };
        }
        if movement.player_mode == Some(MovePlayerMode::Normal)
            && (actor.status.age_ticks == 0 || !player_target_resident)
        {
            actor.apply_direct_player_move(received);
        } else {
            actor.start_movement_interpolation(
                received,
                duration,
                movement.interpolation.force_completion,
                movement.teleported,
            );
        }
        actor.movement_revision = sequence;
        actor.teleported = movement.teleported;
        actor.player_mode = movement.player_mode;
        actor.source_tick = movement.source_tick;
        let dismounted =
            (movement.player_mode == Some(MovePlayerMode::Teleport)).then_some(actor.unique_id);
        self.effect_members.collidable.update(actor);
        if let Some(unique_id) = dismounted {
            self.unlink(unique_id);
        }
        if movement.teleported {
            self.animation.mark_reset(movement.runtime_id);
            if let Some(lifetime) = self.lifetime(movement.runtime_id) {
                self.actions.reset_on_teleport(lifetime);
            }
        }
        ActorApplyResult::Updated
    }
}
