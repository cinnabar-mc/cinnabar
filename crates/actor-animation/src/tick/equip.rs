//! Retained held-stack identity and its lowering and raising transition.
use super::*;

// Vanilla held-item tick: ±0.4 clamp and cached
// stack replacement at height <= 0.1.
const ARM_HEIGHT_STEP: f32 = 0.4;
const ARM_SWAP_HEIGHT: f32 = 0.1;

/// Identifier changes request the native lowering transition. Item-specific stack
/// equivalence/instant-update predicates need stack data beyond the current actor feed.
pub(super) fn advance_height(
    equipped: &mut (Option<Arc<str>>, StackVariant),
    (requested, variant): (&Option<Arc<str>>, StackVariant),
    previous_height: f32,
) -> f32 {
    let swapping = equipped.0 != *requested || (requested.is_some() && equipped.1 != variant);
    let target = if swapping { 0.0 } else { 1.0 };
    let height =
        previous_height + (target - previous_height).clamp(-ARM_HEIGHT_STEP, ARM_HEIGHT_STEP);
    if swapping && height <= ARM_SWAP_HEIGHT {
        *equipped = (requested.clone(), variant);
    }
    height
}
