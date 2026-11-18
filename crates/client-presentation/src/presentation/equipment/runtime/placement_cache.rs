//! Retained attachment placement keyed by its local pose and owner skeleton inputs.
use super::*;
use actor_animation::BoneTransform;

/// Only inputs consumed while composing an already evaluated attachment under its owner.
pub(super) struct Inputs<'a> {
    pub assets: &'a Arc<RuntimeEntityAssets>,
    pub bones: &'a BodyBones,
    pub owner_names: &'a [Box<str>],
    /// The owner's controller layers; roots may follow bones of the models they draw.
    pub owner_layers: &'a [actor_animation::RenderTextureLayer],
    pub geometry: (u32, u32),
    pub local: &'a [BoneTransform],
    pub hidden: &'a Arc<[u32]>,
    pub scale: [f32; 3],
    pub alpha: Option<f32>,
    pub paired: bool,
}

struct Entry {
    lifetime: (u64, i32, u64),
    assets: Arc<RuntimeEntityAssets>,
    bones: BodyBones,
    owner_names: Vec<Box<str>>,
    owner_layers: Vec<OwnerLayer>,
    geometry: (u32, u32),
    local: Vec<BoneTransform>,
    hidden: Arc<[u32]>,
    scale: [u32; 3],
    alpha: Option<u32>,
    paired: bool,
    owner: [RenderPose; 2],
    poses: [RenderPose; 2],
    frame: u64,
}

/// Entries expire with the ordinary equipment pose retention policy.
#[derive(Default)]
pub(super) struct PlacementCache {
    entries: bevy::platform::collections::HashMap<(u64, bool, u8), Entry>,
}

impl PlacementCache {
    /// Releases placements that no recent publication used.
    pub(super) fn retain(&mut self, frame: u64) {
        let oldest = frame.saturating_sub(POSE_MEMO_RETENTION_FRAMES);
        self.entries.retain(|_, entry| entry.frame >= oldest);
    }

    /// Skips parent interpolation and matrix reconstruction when every dependency matches.
    pub(super) fn sample(
        &mut self,
        body: &ActorRigSubmission,
        layer: (bool, u8),
        frame: u64,
        inputs: Inputs<'_>,
        mut transform: impl FnMut(usize, usize) -> Option<RenderBoneTransform>,
    ) -> Option<[RenderPose; 2]> {
        let Inputs {
            assets,
            bones,
            owner_names,
            owner_layers,
            geometry,
            local,
            hidden,
            scale,
            alpha,
            paired,
        } = inputs;
        let identity = body.input.identity;
        let lifetime = (
            identity.session_id,
            identity.dimension,
            identity.spawn_revision.get(),
        );
        let owner = [&body.input.previous_bones, &body.input.current_bones];
        let key = (identity.runtime_id, layer.0, layer.1);
        if let Some(entry) = self.entries.get_mut(&key)
            && entry.lifetime == lifetime
            && Arc::ptr_eq(&entry.assets, assets)
            && entry.bones == *bones
            && entry.owner_names == owner_names
            && entry.owner_layers.len() == owner_draws(owner_layers).count()
            && entry
                .owner_layers
                .iter()
                .zip(owner_draws(owner_layers))
                .all(|(a, b)| a.same(b))
            && entry.geometry == geometry
            && entry.hidden == *hidden
            && entry.scale == scale.map(f32::to_bits)
            && entry.alpha == alpha.map(f32::to_bits)
            && entry.paired == paired
            && entry.local.len() == local.len()
            && entry
                .local
                .iter()
                .zip(local)
                .all(|(a, b)| local_equal(*a, *b))
            && entry
                .owner
                .iter()
                .zip(owner)
                .all(|(a, b)| Arc::ptr_eq(a, b) || render_equal(a, b))
        {
            entry.frame = frame;
            return Some(entry.poses.clone());
        }
        self.entries.remove(&key);
        let mut poses: [RenderPose; 2] = [Arc::from([]), Arc::from([])];
        for endpoint in 0..if paired { 2 } else { 1 } {
            let pose: Vec<_> = (0..local.len())
                .map(|index| transform(endpoint, index))
                .collect::<Option<_>>()?;
            poses[endpoint] = if endpoint == 1 && render_equal(&poses[0], &pose) {
                Arc::clone(&poses[0])
            } else {
                pose.into()
            };
        }
        if !paired {
            poses[1] = Arc::clone(&poses[0]);
        }
        self.entries.insert(
            key,
            Entry {
                lifetime,
                assets: Arc::clone(assets),
                bones: bones.clone(),
                owner_names: owner_names.to_vec(),
                owner_layers: owner_draws(owner_layers).map(OwnerLayer::new).collect(),
                geometry,
                local: local.to_vec(),
                hidden: Arc::clone(hidden),
                scale: scale.map(f32::to_bits),
                alpha: alpha.map(f32::to_bits),
                paired,
                owner: owner.map(Arc::clone),
                poses: poses.clone(),
                frame,
            },
        );
        Some(poses)
    }
}

/// One drawing owner controller layer: the body (`None`), whose poses the owner endpoints
/// already key, or another model and the pose allocations a root may follow.
struct OwnerLayer {
    geometry: Option<u32>,
    previous: Arc<[BoneTransform]>,
    current: Arc<[BoneTransform]>,
}

impl OwnerLayer {
    fn new(layer: &actor_animation::RenderTextureLayer) -> Self {
        Self {
            geometry: layer.geometry,
            previous: Arc::clone(&layer.previous_pose),
            current: Arc::clone(&layer.pose),
        }
    }

    /// Whether `layer` draws the same model, and another model from the same pose allocations.
    fn same(&self, layer: &actor_animation::RenderTextureLayer) -> bool {
        layer.geometry == self.geometry
            && (self.geometry.is_none()
                || Arc::ptr_eq(&self.previous, &layer.previous_pose)
                    && Arc::ptr_eq(&self.current, &layer.pose))
    }
}

/// Owner layers that draw, in controller order. A root takes the first that has its bone, so
/// the body's position among the other models matters as well as their poses.
fn owner_draws(
    layers: &[actor_animation::RenderTextureLayer],
) -> impl Iterator<Item = &actor_animation::RenderTextureLayer> {
    layers.iter().filter(|layer| layer.texture_slot == 0)
}

/// Owner layers drawing their own models, in controller order; only these add root bones.
fn owner_models(
    layers: &[actor_animation::RenderTextureLayer],
) -> impl Iterator<Item = &actor_animation::RenderTextureLayer> {
    owner_draws(layers).filter(|layer| layer.geometry.is_some())
}

/// Floating-point bits retain signed-zero and NaN identity in pose inputs.
fn local_equal(a: BoneTransform, b: BoneTransform) -> bool {
    let BoneTransform {
        rotation,
        translation_scale,
        axis_scale,
    } = a;
    rotation.map(f32::to_bits) == b.rotation.map(f32::to_bits)
        && translation_scale.map(f32::to_bits) == b.translation_scale.map(f32::to_bits)
        && axis_scale.map(f32::to_bits) == b.axis_scale.map(f32::to_bits)
}

/// Compares owner endpoints without requiring stable allocation identities.
fn render_equal(a: &[RenderBoneTransform], b: &[RenderBoneTransform]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            let RenderBoneTransform {
                rotation,
                translation_scale,
                axis_scale,
            } = *a;
            rotation.map(f32::to_bits) == b.rotation.map(f32::to_bits)
                && translation_scale.map(f32::to_bits) == b.translation_scale.map(f32::to_bits)
                && axis_scale.map(f32::to_bits) == b.axis_scale.map(f32::to_bits)
        })
}

/// Whether the owner's body or any controller model a root may follow differs between its
/// previous and current endpoints.
pub(super) fn owner_changing(
    body: &ActorRigSubmission,
    owner_layers: &[actor_animation::RenderTextureLayer],
) -> bool {
    (!Arc::ptr_eq(&body.input.previous_bones, &body.input.current_bones)
        && !render_equal(&body.input.previous_bones, &body.input.current_bones))
        || owner_models(owner_layers).any(|layer| {
            !Arc::ptr_eq(&layer.previous_pose, &layer.pose)
                && (layer.previous_pose.len() != layer.pose.len()
                    || !layer
                        .previous_pose
                        .iter()
                        .zip(layer.pose.iter())
                        .all(|(a, b)| local_equal(*a, *b)))
        })
}

/// Parent interpolation depends on frame time only for an attachment following changing owner
/// bones: the body's, or those of a controller model in `owner_layers` a root may follow.
pub(super) fn alpha(
    body: &ActorRigSubmission,
    owner_layers: &[actor_animation::RenderTextureLayer],
    rig: &actor_animation::AttachableRigSnapshot<'_>,
    geometry: u32,
    count: usize,
    alpha: f32,
) -> Option<f32> {
    (owner_changing(body, owner_layers)
        && (0..count).any(|index| {
            matches!(
                rig.bone_parent(geometry, index),
                Some(
                    actor_animation::AttachableBoneParent::OwnerNamed(_)
                        | actor_animation::AttachableBoneParent::BindingExpression
                )
            )
        }))
    .then_some(alpha)
}
