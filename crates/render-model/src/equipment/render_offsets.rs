//! Held placement of a server-defined item's flat sprite, from its render offsets.

use glam::{Mat4, Quat, Vec3};
use render_api::{
    DEFAULT_ITEM_RENDER_OFFSETS, HeldItemHandSide, HeldItemPerspective, HeldItemPlacement,
    HeldItemTransform, ItemRenderOffsets,
};

use super::display::{ItemDisplay, third_person_sprite_frame};

/// Texels per block, which the default tables' icon size factor is relative to.
const TEXELS_PER_BLOCK: f32 = 16.0;

/// Turns `held_sprite_vertices` axes into native sprite axes. Native extrusion writes column X,
/// depth Y and row Z from zero; the held mesh stores `[-column, height - row, -depth] / long_side`.
/// This proper rotation preserves the UV-labelled front and back faces.
fn native_sprite_rotation() -> Quat {
    Quat::from_rotation_y(std::f32::consts::PI) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
}

/// Native sprite space from held mesh space, in units of the sprite's long side. `None` for an
/// empty sprite.
pub fn native_sprite_from_held(dimensions: [u16; 2]) -> Option<Mat4> {
    let height = sprite_height(dimensions)?;
    Some(
        Mat4::from_translation(Vec3::new(0.0, 0.0, height))
            * Mat4::from_quat(native_sprite_rotation()),
    )
}

/// The sprite's height over its long side; `None` for an empty sprite.
fn sprite_height([width, height]: [u16; 2]) -> Option<f32> {
    (width != 0 && height != 0).then(|| f32::from(height) / f32::from(width.max(height)))
}

/// The sprite's placement from `held_sprite_vertices` space to the frame its table starts from:
/// position, then Z, Y and X turns in degrees, then per-axis scale, with the native sprite frame
/// inside that. Default tables carry an icon size factor, so every icon spans the same length. A
/// declared table scales native texels directly, so a larger icon draws larger. Rotation and scale
/// stay separate because a table may scale an axis to zero. `None` for retail placement or an
/// empty sprite.
pub fn offset_sprite_display(
    placement: HeldItemPlacement,
    hand: HeldItemHandSide,
    perspective: HeldItemPerspective,
    dimensions: [u16; 2],
) -> Option<ItemDisplay> {
    let height = sprite_height(dimensions)?;
    let (offsets, texels): (&ItemRenderOffsets, f32) = match &placement {
        HeldItemPlacement::Retail => return None,
        HeldItemPlacement::DefaultOffsets => (&DEFAULT_ITEM_RENDER_OFFSETS, TEXELS_PER_BLOCK),
        HeldItemPlacement::DeclaredOffsets(offsets) => {
            (offsets, f32::from(dimensions[0].max(dimensions[1])))
        }
    };
    let table: &HeldItemTransform = offsets.transform(hand, perspective);
    let scale = Vec3::from_array(table.scale) * texels;
    let turns = Quat::from_rotation_z(table.rotation[2].to_radians())
        * Quat::from_rotation_y(table.rotation[1].to_radians())
        * Quat::from_rotation_x(table.rotation[0].to_radians());
    Some(ItemDisplay {
        rotation: (turns * native_sprite_rotation()).normalize(),
        translation: Vec3::from_array(table.position)
            + turns * Vec3::new(0.0, 0.0, scale.z * height),
        scale: 1.0,
        // Native X, Y and Z are the held mesh's X, Z and Y.
        axis_scale: Vec3::new(scale.x, scale.z, scale.y),
    })
}

/// The first-person camera-space lead for offset items, applied before their table: the swing's
/// arc and three turns, and the equip dip. A retail item's anchor and 0.4 hand scale do not
/// apply. `swing` is zero during a use animation.
pub fn offset_first_person_lead(swing: f32, equip: f32) -> Mat4 {
    use std::f32::consts::PI;
    let root = swing.max(0.0).sqrt() * PI;
    let (sine, root_sine) = ((swing * PI).sin(), root.sin());
    Mat4::from_translation(Vec3::new(
        root_sine * -0.8,
        (root * 2.0).sin() * 0.1,
        sine * -0.7,
    )) * Mat4::from_translation(Vec3::new(0.0, equip - 1.0, 0.0))
        * Mat4::from_rotation_z((root_sine * 30.0).to_radians())
        * Mat4::from_rotation_y((sine * 20.0).to_radians())
        * Mat4::from_rotation_x((sine * -80.0).to_radians())
}

/// Third-person placement of an offset sprite on its hand's item bone: the retail bone frame and
/// grip, with the table in place of the retail icon transform. `None` for retail placement.
pub fn offset_held_display(
    placement: HeldItemPlacement,
    hand_equipped: bool,
    hand: HeldItemHandSide,
    dimensions: [u16; 2],
) -> Option<ItemDisplay> {
    let table = offset_sprite_display(
        placement,
        hand,
        HeldItemPerspective::ThirdPerson,
        dimensions,
    )?;
    Some(table.under(third_person_sprite_frame(
        hand_equipped,
        hand == HeldItemHandSide::Off,
    )))
}

/// Whether a placement follows the actor's held item scale, which a declared table ignores.
pub fn offset_follows_actor_scale(placement: HeldItemPlacement) -> bool {
    !matches!(placement, HeldItemPlacement::DeclaredOffsets(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equipment::{held_sprite_display_for_hand, sprite_item_transform};
    use glam::Vec4Swizzles;
    use render_api::HeldItemHand;

    const SQUARE_16: [u16; 2] = [16, 16];

    /// Sprite corners in `held_sprite_vertices` space, for 1:1 art.
    const CORNERS: [Vec3; 4] = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    ];

    fn assert_near(actual: Vec3, expected: Vec3, tolerance: f32) {
        assert!(
            actual.distance(expected) <= tolerance,
            "{actual} is not within {tolerance} of {expected}"
        );
    }

    fn display_matrix(display: ItemDisplay) -> Mat4 {
        Mat4::from_translation(display.translation)
            * Mat4::from_quat(display.rotation)
            * Mat4::from_scale(display.axis_scale * display.scale)
    }

    fn offset_sprite_matrix(
        placement: HeldItemPlacement,
        hand: HeldItemHandSide,
        perspective: HeldItemPerspective,
        dimensions: [u16; 2],
    ) -> Option<Mat4> {
        offset_sprite_display(placement, hand, perspective, dimensions).map(display_matrix)
    }

    fn declared_scale(scale: [f32; 3]) -> ItemRenderOffsets {
        let transform = HeldItemTransform {
            position: [0.1, 0.2, 0.3],
            rotation: [10.0, 20.0, 30.0],
            scale,
        };
        let hand = HeldItemHand {
            first_person: transform,
            third_person: transform,
        };
        ItemRenderOffsets {
            main_hand: hand,
            off_hand: hand,
        }
    }

    /// The sprite's top-left texel is the table's origin, so the position is that corner's place.
    #[test]
    fn the_table_position_places_the_sprites_top_left_corner() {
        let matrix = offset_sprite_matrix(
            HeldItemPlacement::DefaultOffsets,
            HeldItemHandSide::Main,
            HeldItemPerspective::FirstPerson,
            SQUARE_16,
        )
        .unwrap();
        assert_near(
            matrix.transform_point3(CORNERS[2]),
            Vec3::new(0.48, -0.128, -0.946),
            1e-5,
        );
    }

    /// Regression: a default-sized server pick must rest where the retail first-person
    /// transforms put a sprite, not at a size or place taken from its icon.
    #[test]
    fn default_first_person_rest_pose_matches_the_retail_sprite() {
        let retail = Mat4::from_translation(Vec3::new(0.56, -0.52, -0.72))
            * Mat4::from_rotation_y(45.0_f32.to_radians())
            * Mat4::from_scale(Vec3::splat(0.4))
            * sprite_item_transform();
        let matrix = offset_first_person_lead(0.0, 1.0)
            * offset_sprite_matrix(
                HeldItemPlacement::DefaultOffsets,
                HeldItemHandSide::Main,
                HeldItemPerspective::FirstPerson,
                SQUARE_16,
            )
            .unwrap();
        for corner in CORNERS {
            assert_near(
                matrix.transform_point3(corner),
                retail.transform_point3(corner),
                0.015,
            );
        }
    }

    #[test]
    fn default_third_person_rest_pose_matches_the_retail_sprite() {
        for (hand_equipped, side) in [
            (true, HeldItemHandSide::Main),
            (false, HeldItemHandSide::Main),
            (true, HeldItemHandSide::Off),
            (false, HeldItemHandSide::Off),
        ] {
            let retail = held_sprite_display_for_hand(hand_equipped, side == HeldItemHandSide::Off);
            let offset = offset_held_display(
                HeldItemPlacement::DefaultOffsets,
                hand_equipped,
                side,
                SQUARE_16,
            )
            .unwrap();
            for corner in CORNERS {
                assert_near(
                    display_matrix(offset).transform_point3(corner),
                    display_matrix(retail).transform_point3(corner),
                    0.015,
                );
            }
        }
    }

    /// Default tables divide by the icon's long side, so 32x32 art spans the same length.
    #[test]
    fn default_tables_ignore_icon_size() {
        let matrix = |dimensions| {
            offset_sprite_matrix(
                HeldItemPlacement::DefaultOffsets,
                HeldItemHandSide::Main,
                HeldItemPerspective::ThirdPerson,
                dimensions,
            )
            .unwrap()
        };
        assert_eq!(matrix(SQUARE_16), matrix([32, 32]));
    }

    /// A declared table scales native texels, so doubling the icon doubles the drawn size.
    #[test]
    fn declared_tables_scale_native_texels() {
        let placement = HeldItemPlacement::DeclaredOffsets(declared_scale([0.05, 0.05, 0.05]));
        let span = |dimensions| {
            let matrix = offset_sprite_matrix(
                placement,
                HeldItemHandSide::Main,
                HeldItemPerspective::FirstPerson,
                dimensions,
            )
            .unwrap();
            matrix
                .transform_point3(CORNERS[0])
                .distance(matrix.transform_point3(CORNERS[2]))
        };
        assert!((span(SQUARE_16) - 0.8).abs() < 1e-5);
        assert!((span([32, 32]) - 1.6).abs() < 1e-5);
    }

    /// Texel X scales the table's X axis and texel Z (the sprite's rows) its Z axis.
    #[test]
    fn non_uniform_scale_acts_on_table_axes() {
        let placement = HeldItemPlacement::DeclaredOffsets(ItemRenderOffsets {
            main_hand: HeldItemHand {
                first_person: HeldItemTransform {
                    position: [0.0; 3],
                    rotation: [0.0; 3],
                    scale: [0.08, 0.04, 0.02],
                },
                third_person: DEFAULT_ITEM_RENDER_OFFSETS.main_hand.third_person,
            },
            off_hand: DEFAULT_ITEM_RENDER_OFFSETS.off_hand,
        });
        let matrix = offset_sprite_matrix(
            placement,
            HeldItemHandSide::Main,
            HeldItemPerspective::FirstPerson,
            SQUARE_16,
        )
        .unwrap();
        // One column left of the top-left corner in held space is one column along native +X.
        let column = matrix.transform_vector3(Vec3::new(-1.0 / 16.0, 0.0, 0.0));
        assert_near(column, Vec3::new(0.08, 0.0, 0.0), 1e-5);
        // One row down in held space is one row along native +Z.
        let row = matrix.transform_vector3(Vec3::new(0.0, -1.0 / 16.0, 0.0));
        assert_near(row, Vec3::new(0.0, 0.0, 0.02), 1e-5);
    }

    /// Rotation and scale stay separate, so the display composes exactly as the table's matrix.
    #[test]
    fn the_display_matches_the_tables_matrix_order() {
        let transform = HeldItemTransform {
            position: [0.3, -0.2, -0.8],
            rotation: [15.0, -30.0, 45.0],
            scale: [0.08, 0.04, 0.02],
        };
        let hand = HeldItemHand {
            first_person: transform,
            third_person: transform,
        };
        let placement = HeldItemPlacement::DeclaredOffsets(ItemRenderOffsets {
            main_hand: hand,
            off_hand: hand,
        });
        let dimensions = [16, 8];
        let expected = Mat4::from_translation(Vec3::from_array(transform.position))
            * Mat4::from_rotation_z(45.0_f32.to_radians())
            * Mat4::from_rotation_y((-30.0_f32).to_radians())
            * Mat4::from_rotation_x(15.0_f32.to_radians())
            * Mat4::from_scale(Vec3::from_array(transform.scale))
            * Mat4::from_scale(Vec3::splat(16.0))
            * native_sprite_from_held(dimensions).unwrap();
        let actual = offset_sprite_matrix(
            placement,
            HeldItemHandSide::Main,
            HeldItemPerspective::FirstPerson,
            dimensions,
        )
        .unwrap();
        for corner in CORNERS {
            assert_near(
                actual.transform_point3(corner),
                expected.transform_point3(corner),
                1e-5,
            );
        }
    }

    /// A zero axis flattens the sprite in place instead of moving it.
    #[test]
    fn a_zero_scale_axis_flattens_without_moving_the_sprite() {
        let placement = HeldItemPlacement::DeclaredOffsets(ItemRenderOffsets {
            main_hand: HeldItemHand {
                first_person: HeldItemTransform {
                    position: [0.1, 0.2, 0.3],
                    rotation: [0.0; 3],
                    scale: [0.05, 0.0, 0.05],
                },
                third_person: DEFAULT_ITEM_RENDER_OFFSETS.main_hand.third_person,
            },
            off_hand: DEFAULT_ITEM_RENDER_OFFSETS.off_hand,
        });
        let matrix = offset_sprite_matrix(
            placement,
            HeldItemHandSide::Main,
            HeldItemPerspective::FirstPerson,
            SQUARE_16,
        )
        .unwrap();
        assert_near(
            matrix.transform_point3(CORNERS[2]),
            Vec3::new(0.1, 0.2, 0.3),
            1e-5,
        );
        assert_near(
            matrix.transform_point3(CORNERS[1]),
            Vec3::new(0.9, 0.2, 0.3),
            1e-5,
        );
    }

    /// A parent frame's rotation and uniform scale reach the display; the table's axes stay its own.
    #[test]
    fn a_parent_frame_keeps_the_tables_axis_scale() {
        let display = ItemDisplay {
            rotation: Quat::from_rotation_z(0.5),
            translation: Vec3::new(1.0, 2.0, 3.0),
            scale: 1.0,
            axis_scale: Vec3::new(2.0, 0.0, 4.0),
        };
        let parent = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0))
            * Mat4::from_rotation_y(1.0)
            * Mat4::from_scale(Vec3::splat(0.5));
        let placed = display.under(parent);
        assert_eq!(placed.axis_scale, display.axis_scale);
        for corner in CORNERS {
            assert_near(
                display_matrix(placed).transform_point3(corner),
                (parent * display_matrix(display)).transform_point3(corner),
                1e-5,
            );
        }
    }

    #[test]
    fn each_hand_and_perspective_selects_its_table() {
        let origin = |hand, perspective| {
            offset_sprite_matrix(
                HeldItemPlacement::DefaultOffsets,
                hand,
                perspective,
                SQUARE_16,
            )
            .unwrap()
            .transform_point3(CORNERS[2])
        };
        let tables = DEFAULT_ITEM_RENDER_OFFSETS;
        assert_near(
            origin(HeldItemHandSide::Off, HeldItemPerspective::FirstPerson),
            Vec3::from_array(tables.off_hand.first_person.position),
            1e-5,
        );
        assert_near(
            origin(HeldItemHandSide::Main, HeldItemPerspective::ThirdPerson),
            Vec3::from_array(tables.main_hand.third_person.position),
            1e-5,
        );
    }

    #[test]
    fn the_lead_rests_at_identity_and_dips_while_equipping() {
        let rest = offset_first_person_lead(0.0, 1.0);
        assert!(rest.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        let dipped = offset_first_person_lead(0.0, 0.25);
        assert_near(dipped.w_axis.xyz(), Vec3::new(0.0, -0.75, 0.0), 1e-6);
    }

    /// Mid-swing, the arc's peak comes from the larger offset-item amplitudes.
    #[test]
    fn the_lead_uses_the_offset_swing_amplitudes() {
        let halfway = offset_first_person_lead(0.25, 1.0);
        let root_sine = (0.5 * std::f32::consts::PI).sin();
        let sine = (0.25 * std::f32::consts::PI).sin();
        assert_near(
            halfway.w_axis.xyz(),
            Vec3::new(root_sine * -0.8, 0.0, sine * -0.7),
            1e-5,
        );
    }

    #[test]
    fn retail_placement_and_empty_sprites_have_no_offset_matrix() {
        assert!(
            offset_sprite_matrix(
                HeldItemPlacement::Retail,
                HeldItemHandSide::Main,
                HeldItemPerspective::FirstPerson,
                SQUARE_16,
            )
            .is_none()
        );
        assert!(
            offset_sprite_matrix(
                HeldItemPlacement::DefaultOffsets,
                HeldItemHandSide::Main,
                HeldItemPerspective::FirstPerson,
                [0, 16],
            )
            .is_none()
        );
    }

    #[test]
    fn only_a_declared_table_ignores_the_actor_scale() {
        assert!(offset_follows_actor_scale(HeldItemPlacement::Retail));
        assert!(offset_follows_actor_scale(
            HeldItemPlacement::DefaultOffsets
        ));
        assert!(!offset_follows_actor_scale(
            HeldItemPlacement::DeclaredOffsets(DEFAULT_ITEM_RENDER_OFFSETS)
        ));
    }
}
