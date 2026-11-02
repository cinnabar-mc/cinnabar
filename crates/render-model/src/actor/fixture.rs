//! Original geometry and artwork for model-part ordering and depth-mask checks.

use super::{ActorRigGeometry, EntityRigId, skin_geometry};

pub const MASK_TEXTURE_SIZE: [u32; 2] = [9, 3];
pub const MASK_TEXTURE_RGBA: [u8; 108] = [
    24, 204, 176, 255, 24, 204, 176, 255, 240, 180, 30, 255, 240, 180, 30, 255, 52, 100, 240, 255,
    52, 100, 240, 255, 180, 60, 220, 255, 180, 60, 220, 255, 224, 45, 60, 255, 24, 204, 176, 255,
    24, 204, 176, 255, 240, 180, 30, 255, 240, 180, 30, 255, 52, 100, 240, 255, 52, 100, 240, 255,
    180, 60, 220, 255, 180, 60, 220, 255, 224, 45, 60, 255, 240, 180, 30, 0, 240, 180, 30, 0, 240,
    180, 30, 0, 240, 180, 30, 0, 240, 180, 30, 0, 240, 180, 30, 0, 240, 180, 30, 0, 240, 180, 30,
    0, 240, 180, 30, 0,
];

/// The visible part precedes its transparent depth masks in the authored array.
pub fn masked_parts() -> ActorRigGeometry {
    let geometry = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.original_mask"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.original_mask","texture_width":9,"texture_height":3},
            "bones":[
                {"name":"z_surface","cubes":[{"origin":[-24,-12,0],"size":[48,24,0],
                    "uv":{"north":{"uv":[0,0],"uv_size":[9,2]}}}]},
                {"name":"a_window","cubes":[
                    {"origin":[-32,-18,-2],"size":[24,36,0],"uv":{"north":{"uv":[0,2],"uv_size":[9,1]}}},
                    {"origin":[8,-18,-2],"size":[24,36,0],"uv":{"north":{"uv":[0,2],"uv_size":[9,1]}}}
                ]}
            ]}]}"#,
    )
    .unwrap()
    .unwrap();
    skin_geometry(&geometry, EntityRigId(0)).unwrap()
}

/// Maps the original model to a 64-pixel view with its masks closer to the camera.
pub fn mask_screen() -> [[f32; 4]; 4] {
    (glam::Mat4::from_translation(glam::Vec3::new(32., 32., 0.5))
        * glam::Mat4::from_scale(glam::Vec3::new(16., -16., -1.)))
    .to_cols_array_2d()
}
