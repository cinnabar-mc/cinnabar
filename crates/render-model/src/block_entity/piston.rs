//! Piston heads: a plate and rod that travel with the extension, and a sleeve that keeps the rod
//! covered inside the body.

use assets::{
    block_entity_geometry::{BoxSpec, textures::AtlasTexture},
    block_shapes::PISTON_DIRECTIONS,
};
use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE},
};

/// The texels the head's box UVs are authored against.
const TEXTURE_SIZE: [f32; 2] = [128.0, 32.0];
/// Distance from the cell centre to the plate's centre when retracted, in blocks.
const RETRACTED_PLATE_OFFSET: f32 = 0.375;
/// The plate's box: a full cell wide and a quarter deep, around the model origin.
const PLATE: BoxSpec = BoxSpec::new([-8.0, -2.0, -8.0], [16.0, 4.0, 16.0], [0.0, 0.0]);
/// The rod's box, reaching from the plate back into the body.
const ROD: BoxSpec = BoxSpec::new([-2.0, 2.0, -2.0], [4.0, 8.0, 4.0], [64.0, 0.0]);
/// The sleeve's box when retracted; it moves back along the rod as the head extends.
const SLEEVE: BoxSpec = BoxSpec::new([-3.0, 2.0, -3.0], [6.0, 8.0, 6.0], [64.0, 18.0]);
/// How far the sleeve moves toward the body base at full extension, in pixels.
const SLEEVE_TRAVEL_PIXELS: f32 = 8.0;

/// A piston's head at one extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PistonArmModel {
    /// `facing_direction`, which indexes `PISTON_DIRECTIONS`.
    pub facing: u8,
    pub sticky: bool,
    /// Extension in `0.0..=1.0`.
    pub progress: f32,
}

impl PistonArmModel {
    /// The point whose block the head is lit from, which is also the plate's centre.
    #[must_use]
    pub fn plate_center(&self, block: [i32; 3]) -> [f32; 3] {
        let center = Vec3::from_array(block.map(|axis| axis as f32 + 0.5));
        (center + self.direction() * (self.progress + RETRACTED_PLATE_OFFSET)).to_array()
    }

    fn direction(&self) -> Vec3 {
        Vec3::from_array(PISTON_DIRECTIONS[usize::from(self.facing)].map(|axis| axis as f32))
    }

    /// Carries the model's axes (+Y pointing from the plate back into the body) into the world.
    fn orientation(&self) -> Mat4 {
        let (x, y, z) = match self.facing {
            0 => (Vec3::X, Vec3::Y, Vec3::Z),
            1 => (Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z),
            2 => (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            3 => (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            4 => (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
            _ => (Vec3::Z, Vec3::X, Vec3::Y),
        };
        Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), Vec3::ZERO.extend(1.0))
    }
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &PistonArmModel,
) {
    let role = if model.sticky {
        AtlasTexture::EntityPistonarmPistonArmSticky
    } else {
        AtlasTexture::EntityPistonarmPistonArm
    };
    let Some(texture) = atlas.texture(role.path(), TEXTURE_SIZE) else {
        return;
    };
    let head = Mat4::from_translation(Vec3::from_array(model.plate_center(block)))
        * model.orientation()
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
    builder.cuboid(Layer::Solid, &texture, head, PLATE, WHITE);
    builder.cuboid(Layer::Solid, &texture, head, ROD, WHITE);
    let sleeve = head * Mat4::from_translation(Vec3::Y * SLEEVE_TRAVEL_PIXELS * model.progress);
    builder.cuboid(Layer::Solid, &texture, sleeve, SLEEVE, WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rod_points_from_the_plate_back_into_the_body_for_every_facing() {
        for facing in 0..6u8 {
            let model = PistonArmModel {
                facing,
                sticky: false,
                progress: 0.0,
            };
            let back = model.orientation().transform_vector3(Vec3::Y);
            assert!(back.abs_diff_eq(-model.direction(), 1.0e-6), "{facing}");
            assert!(
                (model.orientation().determinant() - 1.0).abs() < 1.0e-6,
                "{facing} must turn the model without mirroring it"
            );
        }
    }

    #[test]
    fn the_plate_fills_the_body_gap_when_retracted_and_leaves_it_when_extended() {
        for (progress, expected) in [(0.0, 0.125), (1.0, -0.875)] {
            let model = PistonArmModel {
                facing: 0,
                sticky: false,
                progress,
            };
            let [x, y, z] = model.plate_center([0, 0, 0]);
            assert_eq!([x, z], [0.5, 0.5]);
            assert!((y - expected).abs() < 1.0e-6, "{progress}: {y}");
        }
    }
}
