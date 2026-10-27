//! Static shield model parts shared by dropped items and framed items.
use crate::{DroppedItemBlock, DroppedItemSprite, OPAQUE_WHITE};

/// Shield model-part adjustment after the ordinary framed item's half-scale pose.
const FRAME_SCALE: f32 = 0.64;
const FRAME_TRANSLATION: [f32; 3] = [-0.05, -0.17, 0.025];

/// Applies the shield's static display adjustment inside a framed item's existing face and rotation.
pub fn framed_transform(mut rows: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    for row in &mut rows {
        row[3] += row[0] * FRAME_TRANSLATION[0]
            + row[1] * FRAME_TRANSLATION[1]
            + row[2] * FRAME_TRANSLATION[2];
        for value in &mut row[..3] {
            *value *= FRAME_SCALE;
        }
    }
    rows
}
use assets::gui_item::{
    SHIELD_FACE_CORNERS, SHIELD_MODEL_PART_HEIGHT, SHIELD_MODEL_UNIT, SHIELD_ROOT_BONE,
    shield_face_uvs,
};
use assets::{EntityGeometry, EquipmentTexture, ModelQuad};

/// Visits validated authored faces in the shield's unanimated world coordinate frame.
fn visit_quads(geometry: &EntityGeometry, mut visit: impl FnMut(ModelQuad)) -> Option<()> {
    let bone = geometry
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == SHIELD_ROOT_BONE)?;
    if geometry.inherits.is_some()
        || geometry.texture_width == 0
        || geometry.texture_height == 0
        || bone.parent.is_some()
        || bone.never_render == Some(true)
        || !bone.texture_meshes.is_empty()
        || bone
            .rotation
            .is_some_and(|rotation| rotation.iter().any(|value| value.get() != 0.0))
        || geometry
            .bones
            .iter()
            .any(|bone| bone.parent.as_deref() == Some(SHIELD_ROOT_BONE))
        || bone.cubes.is_empty()
    {
        return None;
    }
    for cube in &bone.cubes {
        if cube.rotation.iter().any(|value| value.get() != 0.0) {
            return None;
        }
        let origin = cube.origin.map(|value| value.get());
        let size = cube.size.map(|value| value.get());
        let inflate = cube.inflate.get();
        let min: [f32; 3] = std::array::from_fn(|axis| origin[axis] - inflate);
        let max: [f32; 3] = std::array::from_fn(|axis| origin[axis] + size[axis] + inflate);
        let mut corners = [[0; 3]; 8];
        for (corner, [x, y, z]) in corners.iter_mut().zip([
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ]) {
            for (fixed, value) in corner.iter_mut().zip([x, y - SHIELD_MODEL_PART_HEIGHT, -z]) {
                let value = (value * SHIELD_MODEL_UNIT + 0.5) * 256.0;
                if !value.is_finite() || value < f32::from(i16::MIN) || value > f32::from(i16::MAX)
                {
                    return None;
                }
                *fixed = value.round() as i16;
            }
        }
        for (indices, uvs) in SHIELD_FACE_CORNERS.into_iter().zip(shield_face_uvs(cube)) {
            let Some(uvs) = uvs else { continue };
            let mut positions =
                indices.map(|index| corners[if cube.mirror { index ^ 1 } else { index }]);
            let mut fixed_uvs = [[0; 2]; 4];
            for (fixed, uv) in fixed_uvs.iter_mut().zip(uvs) {
                for axis in 0..2 {
                    let size = [geometry.texture_width, geometry.texture_height][axis];
                    let value = uv[axis] / f32::from(size);
                    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                        return None;
                    }
                    fixed[axis] = (value * f32::from(assets::MODEL_UV_SCALE)).round() as u16;
                }
            }
            // Reflecting Z reverses every face, independently of the authored mirror.
            positions.reverse();
            fixed_uvs.reverse();
            if cube.mirror {
                positions.reverse();
                fixed_uvs.reverse();
            }
            visit(ModelQuad {
                positions,
                uvs: fixed_uvs,
                material: 0,
                flags: 0,
            });
        }
    }
    Some(())
}

/// Measures the exact validated shield faces without allocating mesh or texture buffers.
pub fn vertex_count(geometry: &EntityGeometry) -> Option<usize> {
    let mut vertices = 0;
    visit_quads(geometry, |quad| {
        vertices += crate::dropped_item::block_quad_vertex_count(&quad)
    })?;
    (vertices > 0).then_some(vertices)
}

/// Retains one full source sheet and the shield's original face UVs for world rendering.
pub fn model(geometry: &EntityGeometry, texture: EquipmentTexture) -> Option<DroppedItemBlock> {
    let mut quads = Vec::new();
    visit_quads(geometry, |quad| quads.push(quad))?;
    if quads.is_empty() {
        return None;
    }
    Some(DroppedItemBlock {
        materials: vec![(
            DroppedItemSprite {
                width: u32::from(texture.width),
                height: u32::from(texture.height),
                rgba8: texture.rgba8,
            },
            OPAQUE_WHITE,
        )]
        .into(),
        quads: quads.into(),
        rotation: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_shield_faces_point_outward_after_coordinate_reflection() {
        let files = vec![("attachables/shield.json".into(), br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":"minecraft:shield","geometry":{"default":"geometry.fixture_shield"},"textures":{"default":"textures/fixture"},"materials":{"default":"entity_alphatest"},"render_controllers":["controller.render.fixture"]}}}"#.to_vec()), ("render_controllers/shield.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","textures":["Texture.default"],"materials":[{"*":"Material.default"}]}}}"#.to_vec()), ("models/shield.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture_shield","texture_width":32,"texture_height":32},"bones":[{"name":"shield","cubes":[{"origin":[-4,20,-1],"size":[8,8,1],"uv":[0,0]}]}]}]}"#.to_vec())];
        let compiled = pack_entity_compiler::compile_entity_pack(files)
            .unwrap()
            .unwrap();
        let geometry = &compiled.assets.geometries[0];
        let texture = EquipmentTexture {
            identifier: "fixture:shield".into(),
            width: 32,
            height: 32,
            rgba8: vec![255; 4096].into(),
        };
        let model = model(geometry, texture).unwrap();
        let center = glam::Vec3::new(128.0, 128.0, 136.0);
        for quad in model.quads.iter() {
            let p = quad
                .positions
                .map(|p| glam::Vec3::from_array(p.map(f32::from)));
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            let midpoint = p.into_iter().sum::<glam::Vec3>() / 4.0;
            assert!(
                normal.dot(midpoint - center) > 0.0,
                "shield lighting requires outward faces"
            );
        }
    }

    #[test]
    fn framed_shield_uses_its_model_part_scale_and_front_plane() {
        let rows = framed_transform(crate::item_frame_item_transform([0; 3], 2, 0.0));
        let point = [1.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let position =
            rows.map(|row| row[3] + row[0] * point[0] + row[1] * point[1] + row[2] * point[2]);
        for (actual, expected) in position.into_iter().zip([0.505, 0.495, 0.905]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        for outward in 0..6 {
            for step in 0..8 {
                let rows = framed_transform(crate::item_frame_item_transform(
                    [1, 2, 3],
                    outward,
                    step as f32 * crate::ITEM_FRAME_ROTATION_STEP_DEGREES,
                ));
                for axis in 0..3 {
                    let length = rows.iter().map(|row| row[axis].powi(2)).sum::<f32>().sqrt();
                    assert!((length - 0.32).abs() < 1e-6);
                }
            }
        }
    }
}
