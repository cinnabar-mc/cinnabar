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

pub const BOX_TEXTURE_SIZE: [u32; 2] = [32, 12];

/// Sparse original artwork leaves transparent RGB behind both planar cube faces.
pub fn box_uv_pixels() -> std::sync::Arc<[u8]> {
    let [width, height] = BOX_TEXTURE_SIZE;
    let colors = [
        [224, 48, 96],
        [32, 184, 216],
        [240, 168, 32],
        [152, 64, 200],
    ];
    (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let region = x * colors.len() as u32 / width;
                let opaque = y < height - 4
                    && match region {
                        0 => true,
                        1 => y % 3 == 0,
                        2 => x % 4 == 0 || y == 3,
                        _ => false,
                    };
                let [r, g, b] = colors[region as usize];
                [r, g, b, if opaque { 255 } else { 0 }]
            })
        })
        .collect::<Vec<_>>()
        .into()
}

/// A planar box keeps coincident front/back faces; the reference owns only its front face.
pub fn box_uv_geometry(front_only: bool) -> ActorRigGeometry {
    let [width, height] = BOX_TEXTURE_SIZE;
    let main_height = height - 4;
    let uv = if front_only {
        serde_json::json!({"north":{"uv":[0,0],"uv_size":[width,main_height]}})
    } else {
        serde_json::json!([0, 0])
    };
    let source = serde_json::json!({"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.original_box_uv","texture_width":width,"texture_height":height},
        "bones":[{"name":"panel","cubes":[{"origin":[-(width as f32)/2.,-(main_height as f32)/2.,0],
            "size":[width,main_height,0],"uv":uv}]}]
    }]}).to_string();
    let geometry = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.original_box_uv"}}"#,
        &source,
    )
    .unwrap()
    .unwrap();
    skin_geometry(&geometry, EntityRigId(0)).unwrap()
}
