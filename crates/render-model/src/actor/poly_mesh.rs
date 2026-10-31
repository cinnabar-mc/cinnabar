//! Converts authored polygon meshes into rig vertices, following vanilla's parse and tessellation.
use assets::{
    EntityGeometryBone, EntityGeometryPolyCorner, EntityGeometryPolyMesh, EntityGeometryScalar,
};

use super::geometry::rotate_euler_around;
use super::{ActorRigGeometryError, ActorRigSurface, ActorRigVertex, MAX_ACTOR_RIG_VERTICES};

/// Fraction of a texel that `adjust_uvs_inward` moves a polygon's edge UVs.
const INWARD_TEXELS: f32 = 0.03125;

/// The bone a mesh belongs to, with what vertices need from its geometry.
struct Part<'a> {
    bone: &'a EntityGeometryBone,
    mesh: &'a EntityGeometryPolyMesh,
    bone_index: u32,
    texture_size: (u16, u16),
}

/// Appends a bone's quads; vanilla compiles them into the part's cube mesh.
pub(super) fn append_quads(
    vertices: &mut Vec<ActorRigVertex>,
    bone: &EntityGeometryBone,
    bone_index: u32,
    texture_size: (u16, u16),
) -> Result<(), ActorRigGeometryError> {
    let Some(mesh) = bone.poly_mesh.as_deref() else {
        return Ok(());
    };
    // Which diagonal vanilla's quad index buffer uses is unverified; it shows only on
    // non-planar quads. Both keep the authored corner order, which sets the facing.
    let corners = mesh
        .quads
        .iter()
        .flat_map(|[first, second, third, fourth]| [first, second, third, first, third, fourth]);
    // Quads negate the normal's Y in vanilla's Y-down part frame, which the rig frame flips back.
    let normal = |[x, y, z]: [f32; 3]| [-x, y, z];
    let part = Part {
        bone,
        mesh,
        bone_index,
        texture_size,
    };
    append(vertices, &part, corners, mesh.quads.len() * 6, normal)
}

/// Appends a bone's triangles; vanilla compiles all of them after every cube and quad.
pub(super) fn append_triangles(
    vertices: &mut Vec<ActorRigVertex>,
    bone: &EntityGeometryBone,
    bone_index: u32,
    texture_size: (u16, u16),
) -> Result<(), ActorRigGeometryError> {
    let Some(mesh) = bone.poly_mesh.as_deref() else {
        return Ok(());
    };
    // Triangles keep the normal's Y in vanilla's Y-down part frame, unlike its quads.
    let normal = |[x, y, z]: [f32; 3]| [-x, -y, z];
    let part = Part {
        bone,
        mesh,
        bone_index,
        texture_size,
    };
    let corners = mesh.triangles.iter().flatten();
    append(vertices, &part, corners, mesh.triangles.len() * 3, normal)
}

/// Emits `added` corners. A mesh that would pass the rig's capacity is skipped and logged, so
/// the rest of the model still draws.
fn append<'a>(
    vertices: &mut Vec<ActorRigVertex>,
    part: &Part<'a>,
    corners: impl Iterator<Item = &'a EntityGeometryPolyCorner>,
    added: usize,
    rig_normal: impl Fn([f32; 3]) -> [f32; 3],
) -> Result<(), ActorRigGeometryError> {
    if added == 0 {
        return Ok(());
    }
    if vertices.len().saturating_add(added) > MAX_ACTOR_RIG_VERTICES {
        tracing::warn!(
            bone = &*part.bone.name,
            added,
            "skipped polygons beyond the actor rig vertex capacity"
        );
        return Ok(());
    }
    let uvs = resolved_uvs(part.mesh, part.texture_size);
    let pivot = part
        .bone
        .pivot
        .map_or([0.0; 3], |pivot| pivot.map(|value| value.get() / 16.0));
    let bind = part
        .bone
        .bind_pose_rotation
        .map(|rotation| rotation.map(EntityGeometryScalar::get))
        .filter(|rotation| rotation.iter().any(|value| *value != 0.0));
    vertices.reserve(added);
    for &[position, normal, uv] in corners {
        let mut position = part.mesh.positions[position as usize].map(|value| value.get() / 16.0);
        let mut normal = part.mesh.normals[normal as usize].map(EntityGeometryScalar::get);
        if let Some(bind) = bind {
            // Vanilla's Y-down part frame turns X and Z the opposite way to the authored frame.
            let authored = [-bind[0], bind[1], -bind[2]];
            position = rotate_euler_around(position, pivot, authored)
                .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
            normal = rotate_euler_around(normal, [0.0; 3], bind)
                .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
        }
        let uv = uvs[uv as usize];
        vertices.push(ActorRigVertex {
            position: [-position[0], position[1], position[2]],
            normal: rig_normal(normal),
            uv,
            back_uv: uv,
            bone_index: part.bone_index,
            surface: ActorRigSurface::SINGLE_FACE,
            glint_origin: [0.0; 2],
        });
    }
    Ok(())
}

/// Authored UVs as vanilla draws them: V flips on parse, edges move inward, and without
/// `normalized_uvs` the value is multiplied by the texture size before clamping. Vanilla does
/// not divide, so pixel coordinates saturate at the texture's far edge.
pub(super) fn resolved_uvs(
    mesh: &EntityGeometryPolyMesh,
    texture_size: (u16, u16),
) -> Vec<[f32; 2]> {
    let size = [f32::from(texture_size.0), f32::from(texture_size.1)];
    let mut uvs: Vec<[f32; 2]> = mesh
        .uvs
        .iter()
        .map(|[u, v]| [u.get(), 1.0 - v.get()])
        .collect();
    if size.iter().all(|size| *size >= 2.0) {
        adjust_uvs_inward(&mut uvs, mesh, size);
    }
    let scale = if mesh.normalized_uvs { [1.0; 2] } else { size };
    for uv in &mut uvs {
        *uv = [0, 1].map(|axis| (uv[axis] * scale[axis]).clamp(0.0, 1.0));
    }
    uvs
}

/// Moves a shared UV towards the centre of its polygons, unless they pull it both ways.
fn adjust_uvs_inward(uvs: &mut [[f32; 2]], mesh: &EntityGeometryPolyMesh, size: [f32; 2]) {
    const DECREASE: u8 = 1;
    const INCREASE: u8 = 2;
    let mut pulls = vec![[0_u8; 2]; uvs.len()];
    let polygons = mesh
        .triangles
        .iter()
        .map(|polygon| &polygon[..])
        .chain(mesh.quads.iter().map(|polygon| &polygon[..]));
    for polygon in polygons {
        for axis in 0..2 {
            let mean = polygon
                .iter()
                .map(|corner| uvs[corner[2] as usize][axis])
                .sum::<f32>()
                * (1.0 / polygon.len() as f32);
            for corner in polygon {
                let value = uvs[corner[2] as usize][axis];
                if mean + f32::EPSILON < value {
                    pulls[corner[2] as usize][axis] |= DECREASE;
                } else if value < mean - f32::EPSILON {
                    pulls[corner[2] as usize][axis] |= INCREASE;
                }
            }
        }
    }
    for (uv, pulls) in uvs.iter_mut().zip(pulls) {
        for axis in 0..2 {
            let step = INWARD_TEXELS / size[axis];
            match pulls[axis] {
                DECREASE => uv[axis] -= step,
                INCREASE => uv[axis] += step,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::EntityRigId;
    use super::*;
    use crate::skin_geometry;

    const STEP: f32 = INWARD_TEXELS / 64.0;

    /// Builds a model through the skin parser, which shares its mesh type with pack entities.
    fn vertices(bones: Value) -> Vec<ActorRigVertex> {
        let source = json!({"format_version": "1.12.0", "minecraft:geometry": [{
            "description": {
                "identifier": "geometry.fixture", "texture_width": 64, "texture_height": 64
            },
            "bones": bones,
        }]});
        let model = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.fixture"}}"#,
            &source.to_string(),
        )
        .unwrap()
        .unwrap();
        skin_geometry(&model, EntityRigId(0))
            .unwrap()
            .vertices
            .to_vec()
    }

    fn square(normalized: bool, uvs: Value, polys: Value) -> Value {
        json!({
            "normalized_uvs": normalized,
            "positions": [[-8, 0, 0], [8, 0, 0], [8, 16, 0], [-8, 16, 0]],
            "normals": [[0, 0, -1], [0, 1, 0], [1, 0, 0], [0, 0, -1]],
            "uvs": uvs,
            "polys": polys,
        })
    }

    fn unit_uvs() -> Value {
        json!([[0, 0], [1, 0], [1, 1], [0, 1]])
    }

    fn quad() -> Value {
        json!([[[0, 0, 0], [1, 0, 1], [2, 0, 2], [3, 0, 3]]])
    }

    fn close(left: [f32; 3], right: [f32; 3]) -> bool {
        (0..3).all(|axis| (left[axis] - right[axis]).abs() < 1.0e-5)
    }

    #[test]
    fn quads_keep_authored_order_and_face_their_authored_normal() {
        let built =
            vertices(json!([{"name": "root", "poly_mesh": square(true, unit_uvs(), quad())}]));
        assert_eq!(built.len(), 6);
        assert_eq!(built[0].position, [0.5, 0.0, 0.0]);
        assert_eq!(built[1].position, [-0.5, 0.0, 0.0]);
        assert_eq!(built[2].position, [-0.5, 1.0, 0.0]);
        assert_eq!(built[5].position, [0.5, 1.0, 0.0]);
        let [first, second, third] = [0, 1, 2].map(|at| glam::Vec3::from(built[at].position));
        let facing = (second - first).cross(third - second).normalize();
        assert_eq!(facing.to_array(), [0.0, 0.0, -1.0]);
        assert_eq!(built[0].normal, [0.0, 0.0, -1.0]);
    }

    #[test]
    fn triangle_normals_keep_the_vanilla_sign_that_differs_from_quads() {
        let mesh = |polys| json!([{"name": "root", "poly_mesh": square(true, unit_uvs(), polys)}]);
        let corners = [[0, 1, 0], [1, 1, 1], [2, 1, 2]];
        assert_eq!(vertices(mesh(json!([corners])))[0].normal, [0.0, -1.0, 0.0]);
        let corners = [[0, 1, 0], [1, 1, 1], [2, 1, 2], [3, 1, 3]];
        assert_eq!(vertices(mesh(json!([corners])))[0].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn triangles_draw_after_every_bone_cube_and_quad() {
        let triangle = json!([[[0, 0, 0], [1, 0, 1], [2, 0, 2]]]);
        let cube = json!([{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]);
        let built = vertices(json!([
            {"name": "a", "poly_mesh": square(true, unit_uvs(), triangle)},
            {"name": "b", "cubes": cube, "poly_mesh": square(true, unit_uvs(), quad())},
        ]));
        assert_eq!(built.len(), 36 + 6 + 3);
        assert!(built[..42].iter().all(|vertex| vertex.bone_index == 1));
        assert!(built[42..].iter().all(|vertex| vertex.bone_index == 0));
        // Native draws both a bone's cubes and its polygons.
        assert_eq!(built[36].position, [0.5, 0.0, 0.0]);
    }

    #[test]
    fn hidden_bones_draw_no_polygons() {
        let built = vertices(json!([
            {"name": "shown", "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]},
            {"name": "hidden", "neverRender": true, "poly_mesh": square(true, unit_uvs(), quad())},
        ]));
        assert_eq!(built.len(), 36);
    }

    #[test]
    fn pixel_uvs_multiply_by_texture_size_and_saturate() {
        let uvs = json!([[0, 64], [64, 64], [64, 0], [0, 0]]);
        let built = vertices(json!([{"name": "root", "poly_mesh": square(false, uvs, quad())}]));
        // V flips first, so authored 64 is negative and clamps to zero. Positive values saturate,
        // and a low edge keeps only its inward shift scaled up by the texture size.
        assert_eq!(built[0].uv, [INWARD_TEXELS, 0.0]);
        assert_eq!(built[1].uv, [1.0, 0.0]);
        assert_eq!(built[2].uv, [1.0, 1.0]);
        assert_eq!(built[5].uv, [INWARD_TEXELS, 1.0]);
    }

    #[test]
    fn small_unnormalized_values_scale_up_rather_than_down() {
        let uvs = json!([
            [0.0, 1.0],
            [0.0078125, 1.0],
            [0.0078125, 0.9921875],
            [0.0, 0.9921875]
        ]);
        let built = vertices(json!([{"name": "root", "poly_mesh": square(false, uvs, quad())}]));
        let far = (0.0078125 - STEP) * 64.0;
        assert!((built[1].uv[0] - far).abs() < 1.0e-4, "{:?}", built[1].uv);
    }

    #[test]
    fn normalized_uvs_flip_v_and_move_edges_inward() {
        let built =
            vertices(json!([{"name": "root", "poly_mesh": square(true, unit_uvs(), quad())}]));
        assert_eq!(built[0].uv, [STEP, 1.0 - STEP]);
        assert_eq!(built[1].uv, [1.0 - STEP, 1.0 - STEP]);
        assert_eq!(built[2].uv, [1.0 - STEP, STEP]);
        assert_eq!(built[5].uv, [STEP, STEP]);
    }

    #[test]
    fn a_uv_pulled_both_ways_by_its_polygons_does_not_move() {
        let polys = json!([
            [[0, 0, 0], [1, 0, 1], [2, 0, 2]],
            [[0, 0, 3], [1, 0, 1], [2, 0, 4]],
        ]);
        let uvs = json!([[0, 0], [0.5, 0], [0, 1], [1, 0], [1, 1]]);
        let built = vertices(json!([{"name": "root", "poly_mesh": square(true, uvs, polys)}]));
        assert_eq!(built[1].uv[0], 0.5);
    }

    #[test]
    fn texture_smaller_than_two_texels_keeps_uvs_exact() {
        let mesh = EntityGeometryPolyMesh {
            normalized_uvs: true,
            uvs: Box::new([[EntityGeometryScalar::ZERO; 2]]),
            triangles: Box::new([[[0, 0, 0]; 3]]),
            ..Default::default()
        };
        assert_eq!(resolved_uvs(&mesh, (1, 64)), [[0.0, 1.0]]);
    }

    #[test]
    fn bind_pose_rotation_turns_positions_and_normals_about_the_pivot() {
        let bone = |bind: Value| {
            json!([{"name": "root", "pivot": [0, 8, 0], "bind_pose_rotation": bind,
                    "poly_mesh": square(true, unit_uvs(), quad())}])
        };
        let flat = vertices(bone(json!([0, 0, 0])));
        assert_eq!(flat[0].position, [0.5, 0.0, 0.0]);
        let turned = vertices(bone(json!([0, 0, 90])));
        // Vanilla's part frame takes (-8, 8, 0) to (-8, -8, 0): authored (-8, 16, 0).
        assert!(
            close(turned[0].position, [0.5, 1.0, 0.0]),
            "{:?}",
            turned[0].position
        );
        assert!(close(turned[0].normal, [0.0, 0.0, -1.0]));
        let turned = vertices(bone(json!([90, 0, 0])));
        assert!(
            close(turned[0].normal, [0.0, 1.0, 0.0]),
            "{:?}",
            turned[0].normal
        );
    }
}
