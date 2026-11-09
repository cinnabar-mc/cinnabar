//! Controller attachments sample the same placement and completed poses as actor rendering.
use super::*;
use render_model::{ActorEmitterTransform, sampled_locator_transform};

/// Resolves an authored locator in a sampled render layer through the placed model; absent or
/// unresolved locators use the actor's interpolated position with the model's axes.
pub fn actor_emitter_transform(
    actor: &ActorSnapshot,
    rig: Option<&ActorRigSnapshot<'_>>,
    layers: &[actor_animation::RenderTextureLayer],
    locator: Option<&str>,
    alpha: f32,
) -> Option<ActorEmitterTransform> {
    let Some(rig) = rig else {
        return Some(ActorEmitterTransform {
            position: actor.interpolated_position(alpha)?,
            basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        });
    };
    let authored = layers
        .iter()
        .find_map(|layer| layer.sampled_scale)
        .unwrap_or([
            rig.scale,
            rig.axis_scale[0],
            rig.axis_scale[1],
            rig.axis_scale[2],
        ]);
    let placement = sampled_rig_placement(rig, actor, alpha, authored)?;
    let world = placement.world_from_model;
    let origin = ActorEmitterTransform {
        position: placement.position,
        basis: std::array::from_fn(|axis| world.map(|row| row[axis])),
    };
    let (Some(name), Some((assets, base))) = (locator, rig.geometry_source()) else {
        return Some(origin);
    };
    for layer in layers
        .iter()
        .rev()
        .map(Some)
        .chain(layers.is_empty().then_some(None))
    {
        let geometry = layer
            .and_then(|layer| layer.geometry)
            .map_or(base, |index| index as usize);
        let Some(binding) = assets.geometry_locator(geometry, name) else {
            continue;
        };
        let index = binding.bone_index as usize;
        let (previous, current) = match layer {
            Some(layer) if !layer.pose.is_empty() => (&*layer.previous_pose, &*layer.pose),
            _ => (rig.previous, rig.current),
        };
        let convert = |bone: &actor_animation::BoneTransform| {
            RenderBoneTransform::from_model_space_scaled(
                bone.rotation,
                bone.translation_scale,
                bone.axis_scale,
            )
        };
        return sampled_locator_transform(
            &binding.locator,
            binding.pivot,
            convert(previous.get(index)?)?,
            convert(current.get(index)?)?,
            world,
            alpha,
        );
    }
    Some(origin)
}
