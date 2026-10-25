//! Tags the draws of entities bound to a programmable world material with its slot, so the
//! renderer shades them with the guest material instead of their Bedrock one.

use crate::presentation::{
    actors::ActorPresentationBatch, entity_layers::ACTOR_LAYER_TEXTURE_BASE,
};
use chunk_pipeline::WorldStream;

/// Marks the body and render-controller layers of every entity whose identifier is bound;
/// equipment, capes, hats and flames keep their own materials, and players are never bound.
pub(super) fn tag(
    batch: &mut ActorPresentationBatch,
    stream: Option<&WorldStream>,
    scene: Option<&render_scene::world_material::WorldMaterialScene>,
) {
    let (Some(stream), Some(scene)) = (stream, scene) else {
        return;
    };
    if scene.set.entities.is_empty() {
        return;
    }
    for submission in &mut batch.submissions {
        let Some(actor) = stream
            .authority()
            .actor(submission.input.identity.runtime_id)
        else {
            continue;
        };
        if let Some((tag, raster)) = world(submission.input.identity.layer, &actor.kind, scene) {
            submission.material.world = tag;
            // The host owns depth, blend and pass: the class's raster replaces the Bedrock one
            // once the material's pipeline is ready, and the draw stays Bedrock's until then.
            submission.material.world_raster = Some(raster);
        }
    }
}

/// The tag and host raster a draw of `layer` of an actor of `kind` takes: an entity's body and
/// render-controller layers when its identifier is bound, nothing otherwise.
fn world(
    layer: u8,
    kind: &protocol::ActorKind,
    scene: &render_scene::world_material::WorldMaterialScene,
) -> Option<(u8, render_api::render_state::RenderState)> {
    if layer != render_model::ACTOR_LAYER_BODY && layer < ACTOR_LAYER_TEXTURE_BASE {
        return None;
    }
    let protocol::ActorKind::Entity { identifier } = kind else {
        return None;
    };
    scene.entity(identifier.as_ref())
}

#[cfg(test)]
mod tests;
