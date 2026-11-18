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

/// Resolves an actor's carried block, using dedicated block-entity geometry when the terrain
/// carrier has no drawable model. Ordinary cubes keep composed carried faces unless retextured.
pub(super) fn carried_block_model(
    cache: &mut ModelCache,
    icons: &UiPresentationRuntime,
    assets: &RuntimeAssets,
    held_models: Option<&PreparedHeldBlockModels>,
    mode: NetworkIdMode,
    id: u32,
) -> Option<u32> {
    let visual = prepared_block_visual(Some((mode, id)), Some(assets));
    let startup_art =
        visual.is_some_and(|visual| !assets.is_retextured(assets::BlockVisualId(visual)));
    let terrain = if startup_art {
        DroppedItemPublisher::carried_block_model(cache, icons, assets, mode, id)
            .or_else(|| DroppedItemPublisher::block_model(cache, assets, mode, id))
    } else {
        DroppedItemPublisher::block_model(cache, assets, mode, id)
            .or_else(|| DroppedItemPublisher::carried_block_model(cache, icons, assets, mode, id))
    };
    terrain.or_else(|| {
        let (identifier, _) = assets::pinned_block_identity(visual?)?;
        let (key, _, block) = model(held_models, identifier)?;
        cache.model(
            key,
            || {
                render_model::dropped_item::ItemModelCost::from_model(&DroppedItemModel::Block(
                    block.clone(),
                ))
            },
            || Some(DroppedItemModel::Block(block.clone())),
        )
    })
}
