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
        submission.material.world = world(submission.input.identity.layer, &actor.kind, scene);
    }
}

/// The tag a draw of `layer` of an actor of `kind` takes: one more than its bound slot for an
/// entity's body and render-controller layers, zero otherwise.
fn world(
    layer: u8,
    kind: &protocol::ActorKind,
    scene: &render_scene::world_material::WorldMaterialScene,
) -> u8 {
    if layer != render_model::ACTOR_LAYER_BODY && layer < ACTOR_LAYER_TEXTURE_BASE {
        return 0;
    }
    let protocol::ActorKind::Entity { identifier } = kind else {
        return 0;
    };
    scene
        .set
        .entity(identifier.as_ref())
        .and_then(|(slot, _)| u8::try_from(slot + 1).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
