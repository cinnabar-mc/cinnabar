//! Vanilla's native brushing flag, which the pack's third-person `brushing` pose reads.

use super::*;
use assets::facts::ItemUseAnimationFact;

/// Vanilla's `variable.is_using_brush`, refreshed for every mob: set while the actor uses a
/// main-hand item whose use animation is the brush's, whichever block it is used on.
pub(super) fn using_brush(
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    input: &ActorTickInput,
) -> bool {
    context.main_hand.is_some()
        && context.main_hand_use_animation == Some(ItemUseAnimationFact::Brush)
        && query::main_hand_elapsed_ticks(actor, context, input).is_some()
}

#[cfg(test)]
#[path = "brush_tests.rs"]
mod tests;
