//! Placement trait parameters and server-owned placing behavior from block definitions.

use super::{CustomPlacement, Nbt};

/// Reads the yaw offset once, applying the trait's quarter-turn rounding.
pub(super) fn traits<'a>(
    traits: impl IntoIterator<Item = &'a Nbt>,
    trait_state_count: usize,
) -> CustomPlacement {
    let offset = traits
        .into_iter()
        .find_map(|trait_| trait_.field("y_rotation_offset").and_then(Nbt::number))
        .unwrap_or(0.0) as f32;
    CustomPlacement {
        trait_state_count,
        y_rotation_offset: if offset.is_finite() {
            if (offset % 90.0).abs() < 0.5 {
                offset
            } else {
                (offset / 90.0 + 0.5).floor().rem_euclid(4.0) * 90.0
            }
        } else {
            0.0
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;

/// A placement filter in any component set applies to the whole block type.
///
/// Server placing handlers (`hasPlayerPlacing`, version 1 custom components or
/// `minecraft:on_player_placing`) do not stop local prediction. The client places the block it
/// holds, and the server's block update corrects a handler that changes or cancels it; without
/// the local block, held bridging aims through cells only the server has filled and keeps placing.
pub(super) fn components(placement: &mut CustomPlacement, components: Option<&Nbt>) {
    let Some(components) = components else { return };
    placement.has_placement_filter |= components.field("minecraft:placement_filter").is_some();
}
