//! Camera-space item placement and first-person arm visibility.

use super::*;
use render_api::{HeldItemHandSide, HeldItemPerspective, HeldItemPlacement};

impl EquipmentRuntime {
    /// The body pose with every bone but the visible arms (and their sleeves) zero-scaled, as
    /// vanilla's first-person part visibility hides them. `None` when no arm shows or the pose
    /// does not match the body geometry.
    pub fn mask_first_person(
        &mut self,
        body: &ActorRigSubmission,
        arms: FirstPersonArms,
    ) -> Option<ActorRigSubmission> {
        if !(arms.right || arms.left) {
            return None;
        }
        let (_, bones) = self.body_bones_for(body.input.rig)?;
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return None;
        }
        let visible = |name: &str| {
            let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
            (arms.right && (is("rightArm") || is("rightSleeve")))
                || (arms.left && (is("leftArm") || is("leftSleeve")))
        };
        let poses = [&body.input.previous_bones, &body.input.current_bones];
        let sampled = self.poses.sample_pair(
            body,
            FIRST_PERSON_MASK_LAYER,
            pose_len,
            |endpoint, index| {
                Some(if visible(&bones.names[index]) {
                    poses[endpoint][index]
                } else {
                    hidden_bone()
                })
            },
        )?;
        let mut masked = body.clone();
        [masked.input.previous_bones, masked.input.current_bones] = sampled;
        Some(masked)
    }

    /// The main-hand item as a first-person layer, when it is drawable. An attachable rides the
    /// posed `rightItem` bone; any other item carries a camera-space bone (`view_space`), placed
    /// by vanilla's first-person transforms for the arm's `hand` state.
    pub fn first_person_item(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: impl Into<FirstPersonHand>,
    ) -> Option<FirstPersonItem> {
        self.first_person_item_for_profile(
            body,
            item,
            hand.into(),
            render_api::DEFAULT_COMBAT_VISUALS,
        )
    }

    /// Uses the selected attack profile for the fallback camera-space item envelope.
    pub fn first_person_item_for_profile(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: FirstPersonHand,
        profile: render_api::CombatVisuals,
    ) -> Option<FirstPersonItem> {
        self.first_person_item_with_use(body, item, hand, profile, Default::default())
    }

    /// Places native stage artwork under the ordinary first-person hand stack.
    pub fn first_person_item_with_use(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: FirstPersonHand,
        profile: render_api::CombatVisuals,
        input: actor_animation::AttachableAnimationInput<'_>,
    ) -> Option<FirstPersonItem> {
        let mesh = self.held_mesh_with_use(item, input)?;
        self.first_person_item_from_mesh(body, item, hand, profile, input, mesh, |runtime| {
            runtime.held_mesh_dimensions(item, input)
        })
    }

    /// Places a custom item's ordinary icon under the first-person hand stack.
    pub fn first_person_item_icon_with_use(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: FirstPersonHand,
        profile: render_api::CombatVisuals,
        input: actor_animation::AttachableAnimationInput<'_>,
    ) -> Option<FirstPersonItem> {
        let (mesh, location, span) = self.held_icon_mesh(item)?;
        self.first_person_item_from_mesh(
            body,
            item,
            hand,
            profile,
            input,
            (mesh, location, false, span),
            |runtime| runtime.held_icon_dimensions(item),
        )
    }

    /// Places an admitted mesh under the native first-person hand transform. `dimensions` is the
    /// texel size of the sprite that mesh draws.
    #[allow(clippy::too_many_arguments)]
    fn first_person_item_from_mesh(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
        hand: FirstPersonHand,
        profile: render_api::CombatVisuals,
        input: actor_animation::AttachableAnimationInput<'_>,
        (mesh, location, block, span): (EntityRigId, ActorArtworkLocation, bool, [f32; 2]),
        dimensions: impl FnOnce(&Self) -> Option<[u16; 2]>,
    ) -> Option<FirstPersonItem> {
        let (_, bones) = self.body_bones_for(body.input.rig)?;
        let pose_len = bones.names.len();
        if body.input.previous_bones.len() != pose_len || body.input.current_bones.len() != pose_len
        {
            return None;
        }
        let shape = if block {
            FirstPersonShape::Block
        } else {
            FirstPersonShape::Sprite {
                mirrored_art: item.grip.mirrored_art(),
            }
        };
        if !hand.swing.is_finite() || !hand.equip.is_finite() {
            return None;
        }
        let hand = if hand.consume.is_none()
            && !matches!(shape, FirstPersonShape::Sprite { mirrored_art: true })
        {
            FirstPersonHand {
                swing: hand.swing.clamp(0.0, 1.0),
                equip: hand.equip.clamp(0.0, 1.0),
                ..hand
            }
        } else {
            hand
        };
        let placement = push::held_sprite_placement(item, block);
        let bone = if placement == HeldItemPlacement::Retail {
            let mut bone = view_bone(super::super::display::first_person_display_for_profile(
                shape, hand, profile,
            ))?;
            bone.translation_scale[3] *= input.main_hand_scale();
            bone
        } else {
            let table = render_model::equipment::offset_sprite_display(
                placement,
                HeldItemHandSide::Main,
                HeldItemPerspective::FirstPerson,
                dimensions(self)?,
                input.main_hand_scale(),
            )?;
            view_bone(super::super::display::first_person_offset_display(
                table, hand,
            ))?
        };
        let poses = self
            .poses
            .share(body, FIRST_PERSON_ITEM_LAYER, [&[bone], &[bone]]);
        Some(FirstPersonItem {
            presentation: push::held_glint(
                layer_presentation(body, LAYER_MAIN_HAND, mesh, poses, location, 0),
                item,
                span,
            ),
            camera_space: true,
            alpha_mode: self.first_person_alpha_mode(item, block),
            java_camera: None,
            java_normal_axis: bevy::math::Vec3::Z,
        })
    }

    /// Ordinary offhand geometry follows the separate native offhand stack.
    pub fn first_person_offhand(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
    ) -> Option<FirstPersonItem> {
        let input = actor_animation::AttachableAnimationInput {
            off_hand: true,
            ..Default::default()
        };
        let (mesh, location, block, span) = self.held_mesh_with_use(item, input)?;
        let equipped = item.grip.upright().unwrap_or(false);
        let bone = if block {
            super::super::first_person::offhand_pose(equipped, true)
        } else {
            let dimensions = self.held_mesh_dimensions(item, input)?;
            offhand_sprite_bone(item, equipped, dimensions)
        }?;
        let poses = self
            .poses
            .share(body, FIRST_PERSON_OFFHAND_LAYER, [&[bone], &[bone]]);
        Some(FirstPersonItem {
            presentation: push::held_glint(
                layer_presentation(body, LAYER_OFF_HAND, mesh, poses, location, 0),
                item,
                span,
            ),
            camera_space: true,
            alpha_mode: self.first_person_alpha_mode(item, block),
            java_camera: None,
            java_normal_axis: bevy::math::Vec3::Z,
        })
    }

    /// Places a custom item's ordinary icon in the native offhand pose.
    pub fn first_person_offhand_icon(
        &mut self,
        body: &ActorRigSubmission,
        item: &WornItem,
    ) -> Option<FirstPersonItem> {
        let (mesh, location, span) = self.held_icon_mesh(item)?;
        let equipped = item.grip.upright().unwrap_or(false);
        let dimensions = self.held_icon_dimensions(item)?;
        let bone = offhand_sprite_bone(item, equipped, dimensions)?;
        let poses = self
            .poses
            .share(body, FIRST_PERSON_OFFHAND_LAYER, [&[bone], &[bone]]);
        Some(FirstPersonItem {
            presentation: push::held_glint(
                layer_presentation(body, LAYER_OFF_HAND, mesh, poses, location, 0),
                item,
                span,
            ),
            camera_space: true,
            alpha_mode: self.first_person_alpha_mode(item, false),
            java_camera: None,
            java_normal_axis: bevy::math::Vec3::Z,
        })
    }
}

/// The first-person off-hand bone of a flat sprite: its render offsets table with no lead of its
/// own and no held item scale, or the retail off-hand pose.
fn offhand_sprite_bone(
    item: &WornItem,
    equipped: bool,
    dimensions: [u16; 2],
) -> Option<render_model::RenderBoneTransform> {
    let placement = push::held_sprite_placement(item, false);
    if placement == HeldItemPlacement::Retail {
        return super::super::first_person::offhand_sprite_pose(equipped, dimensions);
    }
    view_bone(render_model::equipment::offset_sprite_display(
        placement,
        HeldItemHandSide::Off,
        HeldItemPerspective::FirstPerson,
        dimensions,
        1.0,
    )?)
}
