//! Admits dedicated item geometry from the currently installed block-entity artwork.

use super::*;
use render_model::block_entity::PreparedHeldBlockModels;

/// Keys a dedicated startup model by its source after earlier artwork routes were refused.
pub(super) fn model<'a>(
    source: Option<&'a PreparedHeldBlockModels>,
    identifier: &str,
) -> Option<(ModelKey, DroppedItemShape, &'a DroppedItemBlock)> {
    let source = source?;
    let (rig, shape, model) = source.dropped_model(identifier)?;
    Some((
        ModelKey::HeldBlock {
            artwork: source.identity(),
            rig,
        },
        shape,
        model,
    ))
}
