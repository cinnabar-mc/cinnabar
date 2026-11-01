use bevy::log::info;
use client_world::CommittedControlEvent;

use crate::runtime::telemetry::bedrock_camera_rotation;
use client_presentation::camera::CameraSettingsAuthority;
use client_presentation::local_player::LocalViewPose;

/// A local-player MovePlayer rotation offered to granted mods, in Bedrock degrees.
pub(crate) struct MovePlayerRotation {
    pub mode: protocol::MovePlayerMode,
    pub server: [f32; 2],
    pub presented: [f32; 2],
}

/// Applies one committed local-player control event to the view pose, camera
/// authority, and pending spawn anchor.
///
/// MovePlayer owns rotation, which `rotation_filter` may replace; position always follows
/// the server. Prediction corrections only reconcile movement.
pub(crate) fn apply_committed_control(
    control: CommittedControlEvent,
    view: &mut LocalViewPose,
    camera_settings: &mut CameraSettingsAuthority,
    pending_surface_spawn: &mut Option<[i32; 2]>,
    rotation_filter: &mut dyn FnMut(MovePlayerRotation) -> Option<[f32; 2]>,
) {
    let resolved = match control {
        CommittedControlEvent::LocalActorMove { resolved, .. } => resolved,
        CommittedControlEvent::MovePlayer {
            movement, resolved, ..
        } => {
            info!(
                runtime_id = movement.runtime_id,
                position = ?movement.position,
                "applying committed local MovePlayer"
            );
            apply_move_player_rotation(&movement, view, rotation_filter);
            resolved
        }
        CommittedControlEvent::PlayerMovementCorrection {
            correction,
            resolved,
            ..
        } => {
            info!(
                tick = correction.tick,
                position = ?correction.position,
                "applying committed server-authoritative movement correction"
            );
            resolved
        }
        CommittedControlEvent::ChangeDimension { resolved, .. } => {
            camera_settings.reset_perspective();
            view.set_freelook(false);
            resolved
        }
        CommittedControlEvent::Respawn {
            respawn, resolved, ..
        } => {
            log_respawn(respawn);
            if !respawn.ready_to_spawn() {
                return;
            }
            resolved
        }
        // The rider snapped to its corrected seat; the view keeps its position source.
        CommittedControlEvent::VehicleMovementCorrection { .. } => {
            view.reanchor_camera();
            return;
        }
        CommittedControlEvent::SetTime { .. }
        | CommittedControlEvent::DimensionChangeAck { .. }
        | CommittedControlEvent::WorldClocks { .. }
        | CommittedControlEvent::DaylightCycle { .. }
        | CommittedControlEvent::WeatherCycle { .. }
        | CommittedControlEvent::Weather { .. }
        | CommittedControlEvent::LocalMovementEffect { .. }
        | CommittedControlEvent::LocalMovementSpeed { .. }
        | CommittedControlEvent::LocalMovementFlags { .. }
        | CommittedControlEvent::LocalGameMode { .. }
        | CommittedControlEvent::NetworkStackLatency { .. }
        | CommittedControlEvent::LocalActorMotion { .. }
        | CommittedControlEvent::LocalMovementBoost { .. }
        | CommittedControlEvent::LocalDeath { .. }
        | CommittedControlEvent::LocalHurt { .. }
        | CommittedControlEvent::PlayerListChanged { .. }
        | CommittedControlEvent::ClientInputLocks { .. } => return,
    };
    view.reanchor_camera();
    view.set_eye_translation(bevy::prelude::Vec3::from_array(resolved.position));
    *pending_surface_spawn = resolved.surface_anchor;
}

/// Applies only the aim of a committed local MovePlayer whose position physics eases,
/// keeping the eye and camera history continuous.
pub(crate) fn apply_committed_aim(
    control: CommittedControlEvent,
    view: &mut LocalViewPose,
    rotation_filter: &mut dyn FnMut(MovePlayerRotation) -> Option<[f32; 2]>,
) {
    if let CommittedControlEvent::MovePlayer { movement, .. } = control {
        apply_move_player_rotation(&movement, view, rotation_filter);
    }
}

/// MovePlayer owns the view rotation, which `rotation_filter` may replace.
fn apply_move_player_rotation(
    movement: &protocol::MovePlayerEvent,
    view: &mut LocalViewPose,
    rotation_filter: &mut dyn FnMut(MovePlayerRotation) -> Option<[f32; 2]>,
) {
    if movement.yaw.is_finite() && movement.pitch.is_finite() {
        let presented = bedrock_view_degrees(view.rotation());
        let applied = rotation_filter(MovePlayerRotation {
            mode: movement.mode,
            server: [movement.yaw, movement.pitch],
            presented,
        })
        .unwrap_or([movement.yaw, movement.pitch]);
        // Keeping the presented view leaves it untouched rather than round-tripping it.
        if applied != presented {
            view.set_rotation(bedrock_camera_rotation(applied[0], applied[1]));
        }
    }
}

/// Bedrock yaw and pitch degrees of a view rotation; the inverse of [`bedrock_camera_rotation`].
fn bedrock_view_degrees(rotation: bevy::prelude::Quat) -> [f32; 2] {
    let (yaw, pitch, _) = rotation.to_euler(bevy::prelude::EulerRot::YXZ);
    [
        (180.0 - yaw.to_degrees()).rem_euclid(360.0),
        -pitch.to_degrees(),
    ]
}

pub(super) fn log_respawn(respawn: protocol::RespawnEvent) {
    info!(
        state = respawn.state,
        runtime_entity_id = respawn.runtime_entity_id,
        position = ?respawn.position,
        "applying committed Respawn"
    );
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{EulerRot, Quat, Vec3};
    use {
        super::*, client_presentation::camera::CameraSettingsAuthority,
        client_presentation::local_player::LocalViewPose,
    };

    /// Passes every control through unchanged, as a client without teleport filters does.
    fn server_rotation(_: MovePlayerRotation) -> Option<[f32; 2]> {
        None
    }

    #[test]
    fn rotation_filter_replaces_only_rotation_and_always_reconciles_position() {
        use protocol::MovePlayerMode::*;
        let aim = Quat::from_euler(EulerRot::YXZ, 0.35, -0.2, 0.0);
        let server_aim = bedrock_camera_rotation(90.0, 15.0);
        for enabled in [false, true] {
            for mode in [Normal, Reset, Teleport, Rotation, Unknown(9)] {
                let mut view = LocalViewPose::new(Vec3::ZERO, aim);
                let mut settings = CameraSettingsAuthority::default();
                let mut offered = None;
                let mut keep_teleport_aim = |rotation: MovePlayerRotation| {
                    offered = Some((rotation.mode, rotation.server));
                    (enabled && rotation.mode == Teleport).then_some(rotation.presented)
                };
                let previous_epoch = view.camera_reanchor_epoch();
                let mut anchor = None;
                apply_committed_control(
                    CommittedControlEvent::MovePlayer {
                        sequence: 1,
                        source_cohort: None,
                        movement: protocol::MovePlayerEvent {
                            yaw: 90.0,
                            pitch: 15.0,
                            mode,
                            ..Default::default()
                        },
                        resolved: client_world::ResolvedServerPosition {
                            position: [3.0, 64.0, 8.0],
                            surface_anchor: Some([3, 8]),
                        },
                    },
                    &mut view,
                    &mut settings,
                    &mut anchor,
                    &mut keep_teleport_aim,
                );
                assert_eq!(offered, Some((mode, [90.0, 15.0])));
                assert_ne!(view.camera_reanchor_epoch(), previous_epoch);
                assert_eq!(view.eye_translation(), Vec3::new(3.0, 64.0, 8.0));
                assert_eq!(anchor, Some([3, 8]));
                let expected = if enabled && mode == Teleport {
                    aim
                } else {
                    server_aim
                };
                assert!(view.rotation().abs_diff_eq(expected, 0.0001));
            }
        }
    }

    #[test]
    fn a_replacement_rotation_turns_the_view_and_presented_matches_the_server_convention() {
        let mut view = LocalViewPose::new(Vec3::ZERO, bedrock_camera_rotation(30.0, -10.0));
        let mut settings = CameraSettingsAuthority::default();
        let mut presented = None;
        apply_committed_control(
            CommittedControlEvent::MovePlayer {
                sequence: 1,
                source_cohort: None,
                movement: protocol::MovePlayerEvent {
                    yaw: 90.0,
                    pitch: 15.0,
                    mode: protocol::MovePlayerMode::Teleport,
                    ..Default::default()
                },
                resolved: client_world::ResolvedServerPosition {
                    position: [3.0, 64.0, 8.0],
                    surface_anchor: None,
                },
            },
            &mut view,
            &mut settings,
            &mut None,
            &mut |rotation| {
                presented = Some(rotation.presented);
                Some([200.0, 45.0])
            },
        );
        let [yaw, pitch] = presented.unwrap();
        assert!((yaw - 30.0).abs() < 0.001 && (pitch + 10.0).abs() < 0.001);
        assert!(
            view.rotation()
                .abs_diff_eq(bedrock_camera_rotation(200.0, 45.0), 0.0001)
        );
    }

    #[test]
    fn a_dimension_change_reanchors_the_camera_at_the_resolved_position() {
        let mut settings = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let previous_epoch = view.camera_reanchor_epoch();
        let mut anchor = None;
        apply_committed_control(
            CommittedControlEvent::ChangeDimension {
                sequence: 1,
                change: protocol::ChangeDimensionEvent {
                    dimension: 1,
                    ..Default::default()
                },
                resolved: client_world::ResolvedServerPosition {
                    position: [3.0, 64.0, 8.0],
                    surface_anchor: Some([3, 8]),
                },
            },
            &mut view,
            &mut settings,
            &mut anchor,
            &mut server_rotation,
        );
        assert_ne!(view.camera_reanchor_epoch(), previous_epoch);
        assert_eq!(view.eye_translation(), Vec3::new(3.0, 64.0, 8.0));
        assert_eq!(anchor, Some([3, 8]));
    }
    #[test]
    fn a_vehicle_snap_reanchors_camera_history_without_moving_the_eye() {
        let mut settings = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::new(Vec3::new(1.0, 2.0, 3.0), Quat::IDENTITY);
        let previous_epoch = view.camera_reanchor_epoch();
        let mut anchor = None;
        apply_committed_control(
            CommittedControlEvent::VehicleMovementCorrection {
                sequence: 1,
                correction: protocol::PlayerMovementCorrectionEvent {
                    position: [40.0, 64.0, 40.0],
                    delta: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    subject: protocol::MovementCorrectionSubject::Vehicle,
                    on_ground: false,
                    tick: 5,
                    vehicle_angular_velocity: None,
                },
                vehicle_runtime_id: 7,
                vehicle_revision: client_world::VehicleRevision { pose: 0, motion: 0 },
            },
            &mut view,
            &mut settings,
            &mut anchor,
            &mut server_rotation,
        );
        assert_ne!(view.camera_reanchor_epoch(), previous_epoch);
        assert_eq!(view.eye_translation(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(anchor, None);
    }

    #[test]
    fn only_ready_respawn_reanchors_camera_motion_history() {
        let mut settings = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let mut anchor = None;
        for state in [0, 1] {
            let previous_epoch = view.camera_reanchor_epoch();
            apply_committed_control(
                CommittedControlEvent::Respawn {
                    sequence: u64::from(state) + 1,
                    respawn: protocol::RespawnEvent {
                        state,
                        position: [3.0, 64.0, 8.0],
                        runtime_entity_id: 1,
                    },
                    resolved: client_world::ResolvedServerPosition {
                        position: [3.0, 64.0, 8.0],
                        surface_anchor: None,
                    },
                },
                &mut view,
                &mut settings,
                &mut anchor,
                &mut server_rotation,
            );
            assert_eq!(view.camera_reanchor_epoch() != previous_epoch, state == 1);
        }
    }
}
