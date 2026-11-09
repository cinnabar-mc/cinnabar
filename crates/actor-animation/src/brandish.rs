//! Vanilla's native trident brandish, which the pack's `brandish_spear` controllers read.

use super::*;
use assets::facts::{ItemTraits, ItemUseAnimationFact};
use protocol_ids::ActorDataKey;

/// Use ticks over which vanilla's brandished spear reaches full charge.
const SPEAR_CHARGE_TICKS: f32 = 10.0;

/// Vanilla's native brandish and its `charge_amount`, refreshed for every mob: using a
/// main-hand item with the spear use animation raises it, charging over ten use ticks; a
/// non-player mob holding a trident it is not using brandishes it while it has a target.
pub(super) fn brandish_spear(
    actor: &dyn AnimatedActor,
    context: &ActorTickContext,
    input: &ActorTickInput,
) -> (bool, f32) {
    if context.main_hand.is_none() {
        return (false, 0.0);
    }
    if let Some(elapsed) = query::main_hand_elapsed_ticks(actor, context, input) {
        return if context.main_hand_use_animation == Some(ItemUseAnimationFact::Spear) {
            (true, (elapsed as f32 / SPEAR_CHARGE_TICKS).clamp(0.0, 1.0))
        } else {
            (false, 0.0)
        };
    }
    // Native compares the raw synched target id with 0 only; vanilla servers clear it to 0,
    // so a -1 id still counts as a target.
    let targeting = matches!(
        actor.metadata().get(&ActorDataKey::Target.wire_value()),
        Some(protocol::ActorMetadataValue::Long(id)) if *id != 0
    );
    let brandishing = !matches!(actor.kind(), protocol::ActorKind::Player { .. })
        && context.held_item_facts[0]
            .traits
            .contains(ItemTraits::TRIDENT)
        && targeting;
    (brandishing, 0.0)
}

#[cfg(test)]
#[path = "brandish_tests.rs"]
mod tests;
