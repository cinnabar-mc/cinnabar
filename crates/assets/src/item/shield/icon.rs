//! Shared shield GUI rasterization for carriers and decorated item stacks.
use crate::gui_item::{
    GUI_ITEM_SIDE, SHIELD_FACE_CORNERS as FACE_CORNERS, SHIELD_ROOT_BONE as ROOT_BONE,
    shield_face_uvs as face_uvs,
};
use crate::{EntityGeometry, EntityGeometryCube, EquipmentTexture, IconSprite};
use std::sync::Arc;
mod raster;
/// Resolution of shield GUI carriers; geometry retains its sixteen-pixel item frame.
const SIDE: usize = 64;
const PIXELS_PER_GUI_PIXEL: f32 = SIDE as f32 / GUI_ITEM_SIDE;
/// Rasterizes the authored shield GUI model at the shared carrier resolution.
pub fn bake(geometry: &EntityGeometry, texture: &EquipmentTexture) -> Option<IconSprite> {
    // The vanilla shield model loads its named root. Exotic animated/inherited model-part trees
    // must not silently use an invented transform; those remain an explicit unsupported branch.
    let bone = geometry
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == ROOT_BONE)?;
    if geometry.inherits.is_some()
        || bone.parent.is_some()
        || bone.never_render == Some(true)
        || !bone.texture_meshes.is_empty()
        || bone
            .rotation
            .is_some_and(|r| r.iter().any(|v| v.get() != 0.0))
        || geometry
            .bones
            .iter()
            .any(|b| b.parent.as_deref() == Some(ROOT_BONE))
        || bone.cubes.is_empty()
    {
        return None;
    }
    let mut pixels = vec![0; SIDE * SIDE * 4];
    for cube in &bone.cubes {
        if cube.rotation.iter().any(|value| value.get() != 0.0) {
            return None;
        }
        append_cube(&mut pixels, geometry, cube, texture);
    }
    Some(IconSprite {
        width: SIDE as u16,
        height: SIDE as u16,
        rgba8: Arc::from(pixels),
    })
}

/// Projects a shield point into the raster carrier while retaining the GUI transform.
fn project(authored: [f32; 3]) -> [f32; 2] {
    let [x, y, _] = crate::gui_item::project_shield(authored);
    [x, y].map(|value| value * PIXELS_PER_GUI_PIXEL)
}

/// Paints one cube in authored face order with the shield material cutoff.
fn append_cube(
    pixels: &mut [u8],
    geometry: &EntityGeometry,
    cube: &EntityGeometryCube,
    texture: &EquipmentTexture,
) {
    let origin = cube.origin.map(|value| value.get());
    let size = cube.size.map(|value| value.get());
    let inflate = cube.inflate.get();
    let min: [f32; 3] = std::array::from_fn(|axis| origin[axis] - inflate);
    let max: [f32; 3] = std::array::from_fn(|axis| origin[axis] + size[axis] + inflate);
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ]
    .map(project);
    let mirror = cube.mirror;
    let uvs = face_uvs(cube);
    for (face, corners_index) in FACE_CORNERS.into_iter().enumerate() {
        let Some(uvs) = uvs[face] else {
            continue;
        };
        let mut points = corners_index.map(|index| corners[if mirror { index ^ 1 } else { index }]);
        let mut uvs = uvs.map(|uv| {
            [
                uv[0] / f32::from(geometry.texture_width),
                uv[1] / f32::from(geometry.texture_height),
            ]
        });
        if mirror {
            points.reverse();
            uvs.reverse();
        }
        // Native GUI disables depth testing, but keeps backface culling. Preserve authored
        // cube/face order (the handle precedes the board), no cube-thumbnail light/depth heuristic.
        for triangle in [[0, 1, 2], [0, 2, 3]] {
            raster::triangle(
                pixels,
                triangle.map(|i| points[i]),
                triangle.map(|i| uvs[i]),
                texture,
            );
        }
    }
}

#[cfg(test)]
mod tests;
