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
        if let Some(bound) = world(submission.input.identity.layer, &actor.kind, scene) {
            submission.material.world = bound.tag;
            // The host owns depth, blend and pass: the class's raster replaces the Bedrock one
            // once the material's pipeline is ready, and the draw stays Bedrock's until then.
            submission.material.world_raster = Some(bound.raster);
            grow_culling_bounds(submission, bound.reach_blocks);
        }
    }
}

/// The binding a draw of `layer` of an actor of `kind` takes: an entity's body and
/// render-controller layers when its identifier is bound, nothing otherwise.
fn world(
    layer: u8,
    kind: &protocol::ActorKind,
    scene: &render_scene::world_material::WorldMaterialScene,
) -> Option<render_scene::world_material::BoundEntity> {
    if layer != render_model::ACTOR_LAYER_BODY && layer < ACTOR_LAYER_TEXTURE_BASE {
        return None;
    }
    let protocol::ActorKind::Entity { identifier } = kind else {
        return None;
    };
    scene.bound_entity(identifier.as_ref())
}

/// Grows `submission`'s culling box by `reach` blocks on every side, so a draw its material
/// moves or extends is not culled while any of it can be seen. The box is in model space, which
/// the instance's smallest axis scale maps to the fewest blocks.
fn grow_culling_bounds(
    submission: &mut render_model::actor::frame::ActorRigSubmission,
    reach: f32,
) {
    if !reach.is_finite() || reach <= 0.0 {
        return;
    }
    let scale = (0..3)
        .map(|axis| {
            (0..3)
                .map(|row| submission.world_from_actor[row][axis].powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(f32::INFINITY, f32::min);
    let grow = reach / scale.max(1.0e-3);
    for extent in &mut submission.culling_bounds.half_extents {
        *extent += grow;
    }
}

#[cfg(test)]
mod tests;
