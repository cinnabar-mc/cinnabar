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
const ROD: BoxSpec = BoxSpec::new([-2.0, -10.0, -2.0], [4.0, 8.0, 4.0], [64.0, 0.0]);
/// The sleeve's box when retracted; it moves back along the rod as the head extends.
const SLEEVE: BoxSpec = BoxSpec::new([-3.0, -10.0, -3.0], [6.0, 8.0, 6.0], [64.0, 18.0]);
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

    /// Carries the model's axes (+Y pointing out of the plate, away from the body) into the
    /// world, so the box unwrap's "up" art faces outward, as the head's art expects.
    fn orientation(&self) -> Mat4 {
        let (x, y, z) = match self.facing {
            0 => (Vec3::NEG_X, Vec3::NEG_Y, Vec3::Z),
            1 => (Vec3::X, Vec3::Y, Vec3::Z),
            2 => (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            3 => (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            4 => (Vec3::Z, Vec3::X, Vec3::Y),
            _ => (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        };
        Mat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            Vec3::ZERO.extend(1.0),
        )
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
    let sleeve = head * Mat4::from_translation(Vec3::NEG_Y * SLEEVE_TRAVEL_PIXELS * model.progress);
    builder.cuboid(Layer::Solid, &texture, sleeve, SLEEVE, WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far the box face drawn from the art rectangle `texels` lies along the head's facing.
    fn face_extent(model: &PistonArmModel, spec: BoxSpec, texels: [f32; 4]) -> f32 {
        let face = spec
            .faces()
            .into_iter()
            .find(|(_, face_texels)| *face_texels == texels)
            .expect("the box has a face with this art");
        let [corner, ..] = face.0;
        let point = model
            .orientation()
            .transform_point3(Vec3::from_array(corner));
        point.dot(model.direction())
    }

    #[test]
    fn the_rod_reaches_from_the_plate_into_the_body_without_mirroring() {
        for facing in 0..6u8 {
            let model = PistonArmModel {
                facing,
                sticky: false,
                progress: 0.0,
            };
            for [x, y, z] in ROD.faces().iter().flat_map(|(corners, _)| *corners) {
                let along = model
                    .orientation()
                    .transform_point3(Vec3::new(x, y, z))
                    .dot(model.direction());
                assert!((-10.0..=-2.0).contains(&along), "{facing}: {along}");
            }
            assert!(
                (model.orientation().determinant() - 1.0).abs() < 1.0e-6,
                "{facing} must turn the model without mirroring it"
            );
        }
    }

    #[test]
    fn the_plates_face_art_points_away_from_the_body_for_every_facing() {
        // The sticky or wooden face is the art at (16, 0)..(32, 16), the box unwrap's "up"
        // face; the side toward the rod is (32, 0)..(48, 16), read bottom to top as "down".
        for facing in 0..6u8 {
            let model = PistonArmModel {
                facing,
                sticky: true,
                progress: 0.0,
            };
            let outer = face_extent(&model, PLATE, [16.0, 0.0, 16.0, 16.0]);
            let inner = face_extent(&model, PLATE, [32.0, 16.0, 16.0, -16.0]);
            assert_eq!((outer, inner), (2.0, -2.0), "{facing}");
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
