//! Offers local-player MovePlayer rotations to granted teleport handlers.
use crate::runtime::world::MovePlayerRotation;
use mod_host::{TeleportContext, TeleportMode, TeleportRotation};

/// The rotation chosen by opted-in components, or none when none handles moves.
pub(crate) fn handle_move_player_rotation(
    mods: Option<&mut super::ModRuntime>,
    rotation: MovePlayerRotation,
) -> Option<[f32; 2]> {
    let hosts = mods?.teleport_handlers()?;
    let degrees = |[yaw, pitch]: [f32; 2]| TeleportRotation { yaw, pitch };
    let handled = hosts.handle_teleport(TeleportContext {
        mode: match rotation.mode {
            protocol::MovePlayerMode::Normal => TeleportMode::Normal,
            protocol::MovePlayerMode::Reset => TeleportMode::Reset,
            protocol::MovePlayerMode::Teleport => TeleportMode::Teleport,
            protocol::MovePlayerMode::Rotation => TeleportMode::Rotation,
            protocol::MovePlayerMode::Unknown(_) => TeleportMode::Other,
        },
        server: degrees(rotation.server),
        presented: degrees(rotation.presented),
        rotation: degrees(rotation.server),
    });
    Some([handled.rotation.yaw, handled.rotation.pitch])
}
