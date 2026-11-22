//! Allow and deny blocks' authority over the column above them.

use assets::{
    NetworkIdMode,
    facts::{ALLOW_BLOCK_IDENTIFIER, DENY_BLOCK_IDENTIFIER},
};
use client_world::game_mode_capabilities::{EditMarker, EditMarkers};
use sim::PaletteWorld;

use crate::movement::PhysicsCollisionRegistries;

/// Reads the marker authority over `cell` from its own column.
///
/// The highest allow or deny marker at or below `cell` governs, as vanilla's block
/// permission checks scan from the edited cell itself down to the dimension minimum; a
/// marker in another column, or above `cell`, does not. Scanning stops at the first
/// unreadable cell, so authority below loaded terrain stays unknown. Whether `cell` is
/// itself a marker is also reported, for its destroy hardness.
pub fn resolve(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    world: &PaletteWorld<'_>,
    cell: [i32; 3],
    minimum_y: i32,
) -> EditMarkers {
    markers_by(cell, minimum_y, |position| {
        let Ok(runtime_id) = world.primary_runtime_id(position) else {
            return Err(());
        };
        Ok(marker(collisions, mode, runtime_id))
    })
}

/// Resolves markers through `marker_at`, where the outer error means the cell is unreadable.
fn markers_by(
    cell: [i32; 3],
    minimum_y: i32,
    mut marker_at: impl FnMut([i32; 3]) -> Result<Option<EditMarker>, ()>,
) -> EditMarkers {
    let target_is_marker = marker_at(cell).ok().flatten().is_some();
    let [x, _, z] = cell;
    let mut column = None;
    let mut y = Some(cell[1]);
    while let Some(height) = y
        && height >= minimum_y
    {
        match marker_at([x, height, z]) {
            Ok(Some(marker)) => {
                column = Some(marker);
                break;
            }
            Ok(None) => {}
            Err(()) => break,
        }
        y = height.checked_sub(1);
    }
    EditMarkers {
        column,
        target_is_marker,
    }
}

/// The marker an admitted runtime identity names, if any.
fn marker(
    collisions: &PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    runtime_id: u32,
) -> Option<EditMarker> {
    match collisions.block_identifier(mode, runtime_id)? {
        ALLOW_BLOCK_IDENTIFIER => Some(EditMarker::Allow),
        DENY_BLOCK_IDENTIFIER => Some(EditMarker::Deny),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
