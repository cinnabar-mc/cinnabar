//! Shared placement of evaluated item bones on an animated owner.
use crate::{
    ActorRigSnapshot, AttachableAnimationInput, AttachableBoneParent, BoneTransform,
    RenderTextureLayer,
};
use assets::RuntimeEntityAssets;
use glam::{Quat, Vec3, Vec4};
use render_model::RenderBoneTransform;

/// The owner bones an attachable root can follow: the drawn body's, and the bones of each
/// render-controller layer that draws its own model.
#[derive(Clone, Copy)]
pub struct OwnerSkeleton<'a> {
    pub previous: &'a [RenderBoneTransform],
    pub current: &'a [RenderBoneTransform],
    /// Bone names of the drawn body, in pose order.
    pub names: &'a [Box<str>],
    /// The owner's controller layers in controller order, and the catalog their models index.
    layers: &'a [RenderTextureLayer],
    catalog: Option<&'a RuntimeEntityAssets>,
}

impl<'a> OwnerSkeleton<'a> {
    /// The drawn body alone.
    pub fn body(
        previous: &'a [RenderBoneTransform],
        current: &'a [RenderBoneTransform],
        names: &'a [Box<str>],
    ) -> Self {
        Self {
            previous,
            current,
            names,
            layers: &[],
            catalog: None,
        }
    }

    /// Adds the controller layers `rig` draws, so roots also find bones only their models have.
    pub fn with_layers(self, rig: &ActorRigSnapshot<'a>) -> Self {
        Self {
            layers: rig.render,
            catalog: rig.animation_variables.asset_catalog(),
            ..self
        }
    }

    /// Whether a root named `name` has an owner bone to follow.
    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// The owner bone `name` resolves to. Vanilla searches each passing render controller's
    /// geometry in controller order and takes the first that has the bone; the body is the
    /// layer drawing the rig's own model, and also answers when no layer has the bone.
    fn find(&self, name: &str) -> Option<OwnerBone<'a>> {
        let body = || {
            self.names
                .iter()
                .position(|owner| owner.eq_ignore_ascii_case(name))
                .map(OwnerBone::Body)
        };
        let Some(catalog) = self
            .catalog
            .filter(|_| self.layers.iter().any(|layer| layer.geometry.is_some()))
        else {
            return body();
        };
        for layer in self.layers.iter().filter(|layer| layer.texture_slot == 0) {
            let found = match layer.geometry {
                None => body(),
                Some(geometry) => {
                    crate::geometry::merged_bone_index(catalog, geometry as usize, name)
                        .filter(|&index| {
                            index < layer.pose.len() && index < layer.previous_pose.len()
                        })
                        .map(|index| {
                            OwnerBone::Layer(&layer.previous_pose[index], &layer.pose[index])
                        })
                }
            };
            if found.is_some() {
                return found;
            }
        }
        body()
    }

    /// The bone's previous and current render transforms.
    fn transforms(&self, bone: OwnerBone<'_>) -> Option<[RenderBoneTransform; 2]> {
        match bone {
            OwnerBone::Body(index) => Some([*self.previous.get(index)?, *self.current.get(index)?]),
            OwnerBone::Layer(previous, current) => {
                let convert = |bone: &BoneTransform| {
                    RenderBoneTransform::from_model_space_scaled(
                        bone.rotation,
                        bone.translation_scale,
                        bone.axis_scale,
                    )
                };
                Some([convert(previous)?, convert(current)?])
            }
        }
    }
}

/// Where an owner holds a named bone.
#[derive(Clone, Copy)]
enum OwnerBone<'a> {
    Body(usize),
    Layer(&'a BoneTransform, &'a BoneTransform),
}

/// Resolves the sampled owner frame inherited by an authored attachable root.
pub fn parent_for_root(
    owner: OwnerSkeleton<'_>,
    input: AttachableAnimationInput<'_>,
    root: AttachableBoneParent<'_>,
    model_scale: [f32; 3],
) -> Option<RenderBoneTransform> {
    let name = match root {
        AttachableBoneParent::Actor => None,
        AttachableBoneParent::OwnerNamed(name) => Some(name),
        AttachableBoneParent::BindingExpression if input.worn => Some(input.item_slot()),
        AttachableBoneParent::BindingExpression if input.off_hand => Some("leftItem"),
        AttachableBoneParent::BindingExpression => Some("rightItem"),
    };
    let mut parent = match name {
        Some(name) => {
            let [previous, current] = owner.transforms(owner.find(name)?)?;
            interpolate_parent(previous, current, input.frame_alpha)?
        }
        None => RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        },
    };
    for (axis, scale) in model_scale.into_iter().enumerate() {
        parent.axis_scale[axis] *= scale;
    }
    Some(parent)
}

/// Interpolates an owner bone in render coordinates.
pub fn interpolate_parent(
    previous: RenderBoneTransform,
    current: RenderBoneTransform,
    alpha: f32,
) -> Option<RenderBoneTransform> {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = Quat::from_vec4(Vec4::from_array(previous.rotation).try_normalize()?);
    let b = Quat::from_vec4(Vec4::from_array(current.rotation).try_normalize()?);
    Some(RenderBoneTransform {
        rotation: a.slerp(b, alpha).to_array(),
        translation_scale: std::array::from_fn(|i| {
            previous.translation_scale[i]
                + (current.translation_scale[i] - previous.translation_scale[i]) * alpha
        }),
        axis_scale: std::array::from_fn(|i| {
            previous.axis_scale[i] + (current.axis_scale[i] - previous.axis_scale[i]) * alpha
        }),
    })
}

/// Vanilla attachable setup copies the parent's complete matrix before the held
/// model's own channels. Poses and translations here already use the mirrored rig frame.
pub fn compose_parent(
    parent: RenderBoneTransform,
    local: BoneTransform,
) -> Option<RenderBoneTransform> {
    let parent_rotation = Quat::from_vec4(Vec4::from_array(parent.rotation).try_normalize()?);
    let rotation = Quat::from_vec4(Vec4::from_array(local.rotation).try_normalize()?);
    let parent_scale = Vec3::from_array(std::array::from_fn(|axis| {
        parent.axis_scale[axis] * parent.translation_scale[3]
    }));
    let offset = Vec3::new(
        local.translation_scale[0],
        local.translation_scale[1],
        local.translation_scale[2],
    ) / 16.0;
    let origin = Vec3::new(
        parent.translation_scale[0],
        parent.translation_scale[1],
        parent.translation_scale[2],
    ) + parent_rotation * (offset * parent_scale);
    let scale = parent_scale * Vec3::from_array(local.axis_scale) * local.translation_scale[3];
    let result = RenderBoneTransform {
        rotation: (parent_rotation * rotation).normalize().to_array(),
        translation_scale: [origin.x, origin.y, origin.z, 1.0],
        axis_scale: [scale.x, scale.y, scale.z, 1.0],
    };
    result.is_finite().then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Supplies independent item and head frames.
    fn owner_parents() -> (Vec<Box<str>>, [RenderBoneTransform; 3]) {
        let names = ["rightItem", "leftItem", "head"].map(Box::from).into();
        let poses = [
            ([3.0, 4.0, 5.0], Quat::from_rotation_x(0.7)),
            ([-2.0, 1.0, 3.0], Quat::from_rotation_z(0.4)),
            ([0.0, 2.0, 0.0], Quat::from_rotation_y(0.9)),
        ]
        .map(|(position, rotation)| RenderBoneTransform {
            rotation: rotation.to_array(),
            translation_scale: [position[0], position[1], position[2], 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        });
        (names, poses)
    }

    #[test]
    fn unbound_attachable_root_keeps_actor_frame_instead_of_either_hand() {
        let (names, poses) = owner_parents();
        let local = BoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 24.0, 20.0, 1.0],
            axis_scale: [1.0; 3],
        };
        for off_hand in [false, true] {
            let parent = parent_for_root(
                OwnerSkeleton::body(&poses, &poses, &names),
                AttachableAnimationInput {
                    off_hand,
                    ..Default::default()
                },
                AttachableBoneParent::Actor,
                [1.0, 2.0, 0.5],
            )
            .unwrap();
            let placed = compose_parent(parent, local).unwrap();
            assert_eq!(placed.translation_scale[..3], [0.0, 3.0, 0.625]);
            assert_eq!(placed.rotation, Quat::IDENTITY.to_array());
            assert_eq!(placed.axis_scale, [1.0, 2.0, 0.5, 1.0]);
        }
    }

    #[test]
    fn named_attachable_root_follows_its_matching_owner_bone() {
        let (names, poses) = owner_parents();
        let parent = parent_for_root(
            OwnerSkeleton::body(&poses, &poses, &names),
            AttachableAnimationInput::default(),
            AttachableBoneParent::OwnerNamed("HEAD"),
            [1.0; 3],
        )
        .unwrap();
        assert_eq!(parent, poses[2]);
    }

    /// A controller layer drawing the fixture's own `geometry.item` model, posed at `pose`.
    fn model_layer(geometry: Option<u32>, pose: BoneTransform) -> RenderTextureLayer {
        let pose: std::sync::Arc<[BoneTransform]> = std::sync::Arc::from([pose]);
        RenderTextureLayer {
            render_state: None,
            masks: None,
            material: Default::default(),
            material_state: None,
            source: 0,
            texture_slot: 0,
            multitexture: None,
            color: [1.0; 4],
            overlay: [0.0; 4],
            hidden_bones: std::sync::Arc::from([]),
            uv_anim: render_model::IDENTITY_UV_ANIM,
            geometry,
            previous_pose: std::sync::Arc::clone(&pose),
            pose,
            ignore_lighting: false,
            light_color_multiplier: 1.0,
            sampled_scale: None,
            runtime_skin: false,
            pack: false,
        }
    }

    #[test]
    fn a_root_follows_the_first_controller_model_that_has_its_bone() {
        let assets = RuntimeEntityAssets::from_compiled(
            crate::test_support::compiled_fixture().with_unresolved_admissions(),
        )
        .unwrap();
        let (names, poses) = owner_parents();
        let raised = BoneTransform {
            rotation: Quat::from_rotation_y(0.3).to_array(),
            translation_scale: [0.0, 1.5, 0.0, 1.0],
            axis_scale: [1.0; 3],
        };
        let expected = RenderBoneTransform::from_model_space_scaled(
            raised.rotation,
            raised.translation_scale,
            raised.axis_scale,
        )
        .unwrap();
        let parent = |layers: &[RenderTextureLayer], root| {
            parent_for_root(
                OwnerSkeleton {
                    layers,
                    catalog: Some(&assets),
                    ..OwnerSkeleton::body(&poses, &poses, &names)
                },
                AttachableAnimationInput::default(),
                root,
                [1.0; 3],
            )
        };
        let hat = model_layer(Some(0), raised);
        let body = model_layer(None, BoneTransform::default());
        // An earlier controller's model has the item bone, so the root follows that model.
        let ahead = [hat.clone(), body.clone()];
        assert_eq!(
            parent(&ahead, AttachableBoneParent::OwnerNamed("rightItem")),
            Some(expected)
        );
        // Bones only the body has still follow the body.
        assert_eq!(
            parent(&ahead, AttachableBoneParent::OwnerNamed("head")),
            Some(poses[2])
        );
        // The body drawn first keeps its own bone.
        let behind = [body, hat];
        assert_eq!(
            parent(&behind, AttachableBoneParent::OwnerNamed("rightItem")),
            Some(poses[0])
        );
        // A bone only a controller model has is found there instead of being lost.
        let only_layer: Vec<Box<str>> = vec!["head".into()];
        assert_eq!(
            parent_for_root(
                OwnerSkeleton {
                    layers: &behind,
                    catalog: Some(&assets),
                    ..OwnerSkeleton::body(&poses[2..], &poses[2..], &only_layer)
                },
                AttachableAnimationInput::default(),
                AttachableBoneParent::BindingExpression,
                [1.0; 3],
            ),
            Some(expected)
        );
    }

    #[test]
    fn slot_expression_attachable_keeps_the_selected_interpolated_hand() {
        let (names, poses) = owner_parents();
        let previous = poses.map(|pose| RenderBoneTransform {
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            ..pose
        });
        for off_hand in [false, true] {
            let parent = parent_for_root(
                OwnerSkeleton::body(&previous, &poses, &names),
                AttachableAnimationInput {
                    off_hand,
                    frame_alpha: 0.5,
                    ..Default::default()
                },
                AttachableBoneParent::BindingExpression,
                [1.0; 3],
            )
            .unwrap();
            let index = usize::from(off_hand);
            assert_eq!(
                parent,
                interpolate_parent(previous[index], poses[index], 0.5).unwrap()
            );
        }
    }

    #[test]
    fn attachable_channels_follow_rotated_parent_not_third_person_grip() {
        let parent = RenderBoneTransform {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array(),
            translation_scale: [1.0, 2.0, 3.0, 2.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        let local = BoneTransform {
            rotation: Quat::from_rotation_z(0.3).to_array(),
            translation_scale: [16.0, 0.0, 0.0, 1.0],
            axis_scale: [1.0, 2.0, 3.0],
        };
        let placed = compose_parent(parent, local).unwrap();
        assert!((placed.translation_scale[0] - 1.0).abs() < 1e-5);
        assert!((placed.translation_scale[1] - 2.0).abs() < 1e-5);
        assert!((placed.translation_scale[2] - 1.0).abs() < 1e-5);
        assert_eq!(placed.axis_scale, [2.0, 4.0, 6.0, 1.0]);
        assert!(Quat::from_array(placed.rotation).abs_diff_eq(
            Quat::from_array(parent.rotation) * Quat::from_array(local.rotation),
            1e-5
        ));
    }

    #[test]
    fn sampled_attachable_parent_does_not_wait_for_the_next_body_tick() {
        let a = RenderBoneTransform {
            rotation: Quat::IDENTITY.to_array(),
            translation_scale: [0.0, 0.0, 0.0, 1.0],
            axis_scale: render_model::UNIT_AXIS_SCALE,
        };
        let b = RenderBoneTransform {
            translation_scale: [4.0, 0.0, 0.0, 1.0],
            ..a
        };
        assert_eq!(
            interpolate_parent(a, b, 0.25).unwrap().translation_scale[0],
            1.0
        );
        assert_eq!(interpolate_parent(a, b, f32::NAN).unwrap(), a);
    }
}
