//! Shared item placement for world equipment and inventory previews.

use crate::RenderBoneTransform;
use glam::{Mat3, Mat4, Quat, Vec3};

/// Item-space to hand-bone placement: rotation, translation in blocks, uniform scale and any
/// per-axis scale in the item's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDisplay {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: f32,
    pub axis_scale: Vec3,
}

impl ItemDisplay {
    /// Decomposes a rotation + uniform scale + translation matrix.
    pub fn from_matrix(matrix: Mat4) -> Self {
        let linear = Mat3::from_mat4(matrix);
        let scale = linear.determinant().cbrt();
        Self {
            rotation: Quat::from_mat3(&(linear * scale.recip())).normalize(),
            translation: matrix.w_axis.truncate(),
            scale,
            axis_scale: Vec3::ONE,
        }
    }

    /// This placement inside a `parent` frame of rotation, uniform scale and translation.
    #[must_use]
    pub fn under(self, parent: Mat4) -> Self {
        let (scale, rotation, _) = parent.to_scale_rotation_translation();
        Self {
            rotation: (rotation * self.rotation).normalize(),
            translation: parent.transform_point3(self.translation),
            scale: self.scale * scale.x,
            axis_scale: self.axis_scale,
        }
    }
}

/// Converts authored degrees to the radians used by matrix rotations.
fn degrees(value: f32) -> f32 {
    value.to_radians()
}

/// The reference's hand-bone frame (Y and X negated) turned into the rig bone frame.
fn rig_from_reference_bone() -> Mat4 {
    Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
}

/// How the third-person hand grips a flat sprite item, as vanilla's humanoid main- and off-hand
/// item placement chooses it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpriteGrip {
    /// Held flat in the palm.
    Flat,
    /// Held upright like a sword: tools, weapons and other hand-equipped items.
    HandEquipped,
    /// Fishing rods, carrots on a stick and warped fungus on a stick, turned over so the line
    /// hangs from the forward tip. Vanilla chooses it ahead of, and instead of, the hand-equipped
    /// grip.
    Rod,
}

impl SpriteGrip {
    /// Chooses the grip from the hand-equipped flag and whether the item is a rod; a rod wins.
    pub const fn new(hand_equipped: bool, rod: bool) -> Self {
        if rod {
            Self::Rod
        } else if hand_equipped {
            Self::HandEquipped
        } else {
            Self::Flat
        }
    }
}

impl From<assets::item::grip::ItemGrip> for SpriteGrip {
    /// Uses the admitted upright flag and rod art; an unresolved grip is held flat. The catalog's
    /// mirrored-rod-art items are exactly the three rods vanilla gives the rod grip.
    fn from(grip: assets::item::grip::ItemGrip) -> Self {
        Self::new(grip.upright().unwrap_or(false), grip.mirrored_art())
    }
}

/// Third-person main-hand placement of a flat sprite item on the `rightItem` bone, from the
/// 26.30 reference's held-item and default item transforms.
pub fn held_sprite_display(grip: SpriteGrip) -> ItemDisplay {
    held_sprite_display_for_hand(grip, false)
}

/// Vanilla's off-hand item has its own bone-frame offset and
/// hand-equipped X translation, rather than reflecting the main-hand grip.
pub fn held_sprite_display_for_hand(grip: SpriteGrip, off_hand: bool) -> ItemDisplay {
    ItemDisplay::from_matrix(third_person_sprite_frame(grip, off_hand) * sprite_item_transform())
}

/// The hand-bone frame and grip a held sprite's own item transform starts from.
pub(super) fn third_person_sprite_frame(grip: SpriteGrip, off_hand: bool) -> Mat4 {
    // Upright grips share their final scale and turns after a grip-specific lead.
    let upright = |lead: Mat4| {
        lead * Mat4::from_scale(Vec3::splat(0.625))
            * Mat4::from_rotation_x(degrees(80.0))
            * Mat4::from_rotation_y(degrees(45.0))
    };
    let grip = match grip {
        SpriteGrip::HandEquipped => upright(
            Mat4::from_rotation_y(degrees(180.0))
                * Mat4::from_translation(Vec3::new(if off_hand { 0.0 } else { 0.1 }, 0.265, 0.0)),
        ),
        SpriteGrip::Rod => upright(
            Mat4::from_rotation_x(degrees(180.0))
                * Mat4::from_rotation_x(degrees(-15.0))
                * Mat4::from_translation(if off_hand {
                    Vec3::new(0.1, 0.265, 0.0)
                } else {
                    Vec3::new(0.1, 0.065, -0.0625)
                }),
        ),
        SpriteGrip::Flat => {
            Mat4::from_translation(Vec3::new(0.3125, 0.1875, -0.1875))
                * Mat4::from_scale(Vec3::splat(0.375))
                * Mat4::from_rotation_z(degrees(60.0))
                * Mat4::from_rotation_x(degrees(-90.0))
                * Mat4::from_rotation_z(degrees(20.0))
        }
    };
    let hand_offset = if off_hand {
        Mat4::from_translation(Vec3::new(-0.125, 0.0, 0.0))
    } else {
        Mat4::IDENTITY
    };
    rig_from_reference_bone() * hand_offset * grip
}

/// Legacy icon transform shared by the item renderer, before its per-view placement.
pub fn sprite_item_transform() -> Mat4 {
    Mat4::from_scale(Vec3::splat(1.5))
        * Mat4::from_rotation_y(degrees(50.0))
        * Mat4::from_rotation_z(degrees(335.0))
        * Mat4::from_translation(Vec3::new(0.075, -0.245, -0.1))
}

/// The item's single bone: the hand bone's pose with `display` applied in the hand frame, so
/// item-space vertices (bind pivot at the origin) land where the hand holds them. `None` for a
/// non-finite pose.
pub fn attach_to_bone(
    hand: RenderBoneTransform,
    display: ItemDisplay,
) -> Option<RenderBoneTransform> {
    let [rx, ry, rz, rw] = hand.rotation;
    let hand_rotation = Quat::from_vec4(glam::Vec4::new(rx, ry, rz, rw).try_normalize()?);
    // A non-uniform hand scale would shear the item; the first axis stands in for it.
    let hand_scale = hand.translation_scale[3] * hand.axis_scale[0];
    let origin = Vec3::new(
        hand.translation_scale[0],
        hand.translation_scale[1],
        hand.translation_scale[2],
    );
    let rotation = (hand_rotation * display.rotation).normalize();
    let translation = origin + hand_rotation * (display.translation * hand_scale);
    let bone = RenderBoneTransform {
        rotation: rotation.to_array(),
        translation_scale: [
            translation.x,
            translation.y,
            translation.z,
            hand_scale * display.scale,
        ],
        axis_scale: [
            display.axis_scale.x,
            display.axis_scale.y,
            display.axis_scale.z,
            1.0,
        ],
    };
    bone.is_finite().then_some(bone)
}

/// Legacy block grip, applied to the centred cube vanilla emits through
/// the (-.5,-.5,-.5) mesh offset.
pub fn held_block_display() -> ItemDisplay {
    held_block_display_for_hand(false)
}

/// A block worn on the head, facing along the head after vanilla's half-turn: the block's
/// default direction (+Z) lies along the head's forward (-Z). The cube size and vertical
/// placement remain provisional.
pub fn head_block_display() -> ItemDisplay {
    ItemDisplay {
        rotation: Quat::from_rotation_y(std::f32::consts::PI),
        translation: Vec3::new(0.0, 0.25, 0.0),
        scale: 0.5625,
        axis_scale: Vec3::ONE,
    }
}

/// Offhand rendering adds its own reference-bone X offset before the shared
/// legacy block display. Custom block presentations use a different native path.
pub fn held_block_display_for_hand(off_hand: bool) -> ItemDisplay {
    let hand_offset = if off_hand {
        Mat4::from_translation(Vec3::new(-0.125, 0.0, 0.0))
    } else {
        Mat4::IDENTITY
    };
    ItemDisplay::from_matrix(
        rig_from_reference_bone()
            * hand_offset
            * Mat4::from_translation(Vec3::new(0.0, 0.1875, -0.3125))
            * Mat4::from_rotation_x(degrees(200.0))
            * Mat4::from_rotation_y(degrees(225.0))
            * Mat4::from_scale(Vec3::splat(0.375)),
    )
}
