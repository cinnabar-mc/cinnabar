//! The bell's swinging body. Terrain draws the frame from the block's own materials.

use std::f32::consts::PI;

use assets::block_entity_geometry::BoxSpec;
use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE},
};

/// Height of the body's pivot in blocks, for a standing frame and for every other mounting.
const STANDING_PIVOT_HEIGHT: f32 = 0.564;
const PIVOT_HEIGHT: f32 = 0.75;
/// Ring animation ticks per game tick, and the count at which the swing ends.
const SWING_TICKS_PER_GAME_TICK: u32 = 2;
const SWING_END_TICK: u32 = 50;
const GAME_TICKS_PER_SECOND: f32 = 20.0;
/// The body's box about its pivot, and the lip below it, as legacy cubes with Y negated (see
/// [`MeshBuilder::legacy_cuboid`]).
const BODY: BoxSpec = BoxSpec::new([-3.0, -1.0, -3.0], [6.0, 7.0, 6.0], [0.0, 0.0]);
const LIP: BoxSpec = BoxSpec::new([-4.0, 6.0, -4.0], [8.0, 2.0, 8.0], [0.0, 13.0]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BellAttachment {
    Standing,
    Hanging,
    Side,
    Multiple,
}

impl BellAttachment {
    #[must_use]
    pub fn from_state(name: &str) -> Option<Self> {
        Some(match name {
            "standing" => Self::Standing,
            "hanging" => Self::Hanging,
            "side" => Self::Side,
            "multiple" => Self::Multiple,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BellModel {
    pub attachment: BellAttachment,
    /// Native direction the last ring struck from: 0 south, 1 west, 2 north, 3 east.
    pub ring_direction: u8,
    /// Seconds since the last ring cue; large values mean at rest.
    pub seconds_since_ring: f32,
}

/// Swing angle in radians `seconds` after a ring. It stops once the ring's ticks run out, and
/// advances two animation ticks per game tick with the partial tick added on top.
#[must_use]
pub fn swing_radians(seconds: f32) -> f32 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0.0;
    }
    let game_ticks = seconds * GAME_TICKS_PER_SECOND;
    let whole_ticks = game_ticks as u32;
    let ticks = whole_ticks.saturating_mul(SWING_TICKS_PER_GAME_TICK);
    if ticks >= SWING_END_TICK {
        return 0.0;
    }
    let time = ticks as f32 + game_ticks.fract();
    (time / PI).sin() / (time / 3.0 + 4.0)
}

/// The body's tilt for a ring from `ring_direction`: east-west strikes tilt about Z and
/// north-south strikes about X, with the far pair turning the other way.
fn swing_matrix(ring_direction: u8, seconds: f32) -> Mat4 {
    let angle = swing_radians(seconds);
    match ring_direction {
        0 => Mat4::from_rotation_x(angle),
        1 => Mat4::from_rotation_z(angle),
        2 => Mat4::from_rotation_x(-angle),
        3 => Mat4::from_rotation_z(-angle),
        _ => Mat4::IDENTITY,
    }
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BellModel,
) {
    let Some(texture) = atlas.texture(
        assets::block_entity_geometry::textures::AtlasTexture::EntityBellBell.path(),
        [32.0, 32.0],
    ) else {
        return;
    };
    let height = if model.attachment == BellAttachment::Standing {
        STANDING_PIVOT_HEIGHT
    } else {
        PIVOT_HEIGHT
    };
    let pivot = Vec3::from_array(block.map(|axis| axis as f32)) + Vec3::new(0.5, height, 0.5);
    let matrix = Mat4::from_translation(pivot)
        * swing_matrix(model.ring_direction, model.seconds_since_ring)
        * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
    for part in [BODY, LIP] {
        builder.legacy_cuboid(Layer::Solid, &texture, matrix, part, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-texture atlas holding the bell's art.
    fn atlas() -> BlockEntityAtlas {
        let placement = assets::BlockEntityPlacement {
            name: assets::block_entity_geometry::textures::AtlasTexture::EntityBellBell
                .path()
                .into(),
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            32,
            32,
            &vec![255; 32 * 32 * 4],
            &[placement],
        )
        .unwrap();
        BlockEntityAtlas::from_assets(&assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap())
    }

    /// The Y range of the emitted body in pixels above the block's base.
    fn height_range(attachment: BellAttachment) -> (f32, f32) {
        let atlas = atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &BellModel {
                attachment,
                ring_direction: 0,
                seconds_since_ring: f32::INFINITY,
            },
        );
        builder
            .solid
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), vertex| {
                let y = vertex.position[1] * 16.0;
                (low.min(y), high.max(y))
            })
    }

    #[test]
    fn a_standing_body_hangs_under_its_lower_bar() {
        let (low, high) = height_range(BellAttachment::Standing);
        assert!((low - 1.024).abs() < 1.0e-3, "lip rim at {low}");
        assert!((high - 10.024).abs() < 1.0e-3, "crown at {high}");
    }

    #[test]
    fn other_mountings_hang_the_body_from_the_high_bar() {
        for attachment in [
            BellAttachment::Hanging,
            BellAttachment::Side,
            BellAttachment::Multiple,
        ] {
            let (low, high) = height_range(attachment);
            assert!((low - 4.0).abs() < 1.0e-3, "lip rim at {low}");
            assert!((high - 13.0).abs() < 1.0e-3, "crown at {high}");
        }
    }

    #[test]
    fn the_crown_and_rim_read_and_shade_as_their_drawn_sides() {
        let atlas = atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &BellModel {
                attachment: BellAttachment::Hanging,
                ring_direction: 0,
                seconds_since_ring: f32::INFINITY,
            },
        );
        // Each quad is two triangles; the crown lies flat at the body's top, the rim's
        // underside at the lip's bottom.
        let flat_at = |pixels: f32| -> Vec<_> {
            builder
                .solid
                .chunks(6)
                .filter(|quad| {
                    quad.iter()
                        .all(|vertex| (vertex.position[1] - pixels / 16.0).abs() < 1.0e-4)
                })
                .flatten()
                .collect()
        };
        let crown = flat_at(13.0);
        let underside = flat_at(4.0);
        assert_eq!(crown.len(), 6, "one crown quad");
        assert_eq!(underside.len(), 6, "one rim underside quad");
        assert!(
            crown[0].color[0] > underside[0].color[0],
            "the crown takes the top shade and the underside the bottom shade"
        );
        let min_u = crown
            .iter()
            .map(|vertex| vertex.uv[0] * atlas.size()[0] as f32)
            .fold(f32::MAX, f32::min);
        assert!(
            (min_u - 12.0).abs() < 1.0e-3,
            "the crown reads the box's down region, starting at u 12, got {min_u}"
        );
    }

    #[test]
    fn the_body_sides_start_their_art_at_the_rim() {
        let atlas = atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &BellModel {
                attachment: BellAttachment::Hanging,
                ring_direction: 0,
                seconds_since_ring: f32::INFINITY,
            },
        );
        // A body side spans the body's full height, 6 to 13 pixels; its art covers rows 6 to 13.
        let sides: Vec<_> = builder
            .solid
            .chunks(6)
            .filter(|quad| {
                let (low, high) = quad
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(low, high), vertex| {
                        let y = vertex.position[1] * 16.0;
                        (low.min(y), high.max(y))
                    });
                (low - 6.0).abs() < 1.0e-3 && (high - 13.0).abs() < 1.0e-3
            })
            .collect();
        assert_eq!(sides.len(), 4, "four body sides");
        for vertex in sides.into_iter().flatten() {
            let y = vertex.position[1] * 16.0;
            let v = vertex.uv[1] * atlas.size()[1] as f32;
            assert!((v - y).abs() < 1.0e-3, "row {v} drawn at height {y}");
        }
    }

    #[test]
    fn the_swing_follows_the_ring_tick_curve_and_ends_on_time() {
        assert_eq!(swing_radians(0.0), 0.0);
        let expected = (8.0_f32 / PI).sin() / (8.0 / 3.0 + 4.0);
        assert!((swing_radians(0.2) - expected).abs() < 1.0e-5);
        assert_eq!(swing_radians(1.25), 0.0);
        assert_eq!(swing_radians(f32::NAN), 0.0);
        assert_eq!(swing_radians(-1.0), 0.0);
    }

    #[test]
    fn rings_tilt_about_the_axis_across_their_strike() {
        let top = Vec3::Y;
        let shifted = |direction| swing_matrix(direction, 0.2).transform_vector3(top);
        assert!(shifted(0).z.abs() > 0.05 && shifted(0).x.abs() < 1.0e-6);
        assert!(shifted(1).x.abs() > 0.05 && shifted(1).z.abs() < 1.0e-6);
        assert!((shifted(0).z + shifted(2).z).abs() < 1.0e-6);
        assert!((shifted(1).x + shifted(3).x).abs() < 1.0e-6);
    }

    #[test]
    fn attachment_names_parse() {
        assert_eq!(
            BellAttachment::from_state("multiple"),
            Some(BellAttachment::Multiple)
        );
        assert_eq!(BellAttachment::from_state("floor"), None);
    }
}
