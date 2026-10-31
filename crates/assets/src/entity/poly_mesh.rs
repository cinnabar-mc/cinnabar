//! Polygon meshes authored on geometry bones, shared by pack entities, attachables and skins.
//!
//! Vanilla reads these leniently: a missing or non-numeric component is zero, an implicit list
//! of any name other than `tri_list` is a quad list, and polygons are not bounds-checked. Out of
//! range indices are undefined there, so they lose only their polygon here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{EntityGeometryScalar, invalid, validate_scalars};
use crate::AssetError;

/// One polygon corner's position, normal and UV indices.
pub type EntityGeometryPolyCorner = [u32; 3];

/// A bone's indexed polygons, with authored attribute values and independent attribute indices.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryPolyMesh {
    pub normalized_uvs: bool,
    pub positions: Box<[[EntityGeometryScalar; 3]]>,
    pub normals: Box<[[EntityGeometryScalar; 3]]>,
    pub uvs: Box<[[EntityGeometryScalar; 2]]>,
    pub triangles: Box<[[EntityGeometryPolyCorner; 3]]>,
    pub quads: Box<[[EntityGeometryPolyCorner; 4]]>,
}

impl EntityGeometryPolyMesh {
    /// Reads a bone's optional `poly_mesh`, logging how many malformed values were skipped.
    /// Returns `None` when the mesh has no polygons to draw.
    #[must_use]
    pub fn parse_bone(value: Option<&Value>, bone: &str) -> Option<Box<Self>> {
        let (mesh, skipped) = Self::parse(value?);
        if skipped > 0 {
            tracing::warn!(bone, skipped, "skipped malformed poly_mesh data");
        }
        mesh.map(Box::new)
    }

    fn parse(value: &Value) -> (Option<Self>, u32) {
        if !value.is_object() {
            return (None, 1);
        }
        let mut skipped = 0;
        let mut mesh = Self {
            normalized_uvs: value
                .get("normalized_uvs")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            positions: attribute(value.get("positions"), &mut skipped),
            normals: attribute(value.get("normals"), &mut skipped),
            uvs: attribute(value.get("uvs"), &mut skipped),
            ..Self::default()
        };
        match value.get("polys") {
            None => {}
            Some(Value::String(mode)) => mesh.parse_implicit_list(mode, &mut skipped),
            Some(Value::Array(polygons)) => mesh.parse_explicit(polygons, &mut skipped),
            Some(_) => skipped += 1,
        }
        let drawable = !mesh.triangles.is_empty() || !mesh.quads.is_empty();
        (drawable.then_some(mesh), skipped)
    }

    /// Reads a list that consumes one position, normal and UV per corner.
    fn parse_implicit_list(&mut self, mode: &str, skipped: &mut u32) {
        let count = self.positions.len();
        let corners = if mode == "tri_list" { 3 } else { 4 };
        if count != self.normals.len() || count != self.uvs.len() || count % corners != 0 {
            *skipped += 1;
            return;
        }
        let Ok(count) = u32::try_from(count) else {
            *skipped += 1;
            return;
        };
        if corners == 3 {
            self.triangles = (0..count / 3)
                .map(|first| [0, 1, 2].map(|at| [first * 3 + at; 3]))
                .collect();
        } else {
            self.quads = (0..count / 4)
                .map(|first| [0, 1, 2, 3].map(|at| [first * 4 + at; 3]))
                .collect();
        }
    }

    /// Reads explicit polygons: three corners form a triangle and any other count a quad.
    fn parse_explicit(&mut self, polygons: &[Value], skipped: &mut u32) {
        let mut triangles = Vec::new();
        let mut quads = Vec::new();
        for polygon in polygons {
            let parsed = polygon.as_array().and_then(|corners| {
                if corners.len() == 3 {
                    triangles.push(self.corners::<3>(corners)?);
                } else {
                    quads.push(self.corners::<4>(corners)?);
                }
                Some(())
            });
            if parsed.is_none() {
                *skipped += 1;
            }
        }
        self.triangles = triangles.into();
        self.quads = quads.into();
    }

    fn corners<const N: usize>(&self, corners: &[Value]) -> Option<[EntityGeometryPolyCorner; N]> {
        let mut parsed = [[0; 3]; N];
        for (at, slot) in parsed.iter_mut().enumerate() {
            for (axis, index) in slot.iter_mut().enumerate() {
                let value = corners.get(at).and_then(|corner| corner.get(axis));
                *index = polygon_index(value)?;
            }
            let [position, normal, uv] = slot.map(|index| index as usize);
            if position >= self.positions.len()
                || normal >= self.normals.len()
                || uv >= self.uvs.len()
            {
                return None;
            }
        }
        Some(parsed)
    }

    /// Checks a carrier's values and polygon indices.
    pub(super) fn validate(&self) -> Result<(), AssetError> {
        self.positions
            .iter()
            .chain(self.normals.iter())
            .try_for_each(validate_scalars)?;
        self.uvs.iter().try_for_each(validate_scalars)?;
        let in_range = |corner: &EntityGeometryPolyCorner| {
            (corner[0] as usize) < self.positions.len()
                && (corner[1] as usize) < self.normals.len()
                && (corner[2] as usize) < self.uvs.len()
        };
        if self.triangles.iter().flatten().all(in_range)
            && self.quads.iter().flatten().all(in_range)
        {
            Ok(())
        } else {
            Err(invalid("entity geometry polygon index is out of range"))
        }
    }

    /// Adds a derived geometry's mesh for the same bone. Inherited attributes keep their indices
    /// and the child's indices shift past them; the child's `normalized_uvs` applies to all.
    pub(super) fn append(&mut self, child: &Self) {
        let offset = [self.positions.len(), self.normals.len(), self.uvs.len()]
            .map(|length| u32::try_from(length).unwrap_or(u32::MAX));
        let shift = |corner: EntityGeometryPolyCorner| {
            std::array::from_fn(|axis| corner[axis].saturating_add(offset[axis]))
        };
        self.normalized_uvs = child.normalized_uvs;
        extend(&mut self.positions, child.positions.iter().copied());
        extend(&mut self.normals, child.normals.iter().copied());
        extend(&mut self.uvs, child.uvs.iter().copied());
        extend(
            &mut self.triangles,
            child.triangles.iter().map(|polygon| polygon.map(shift)),
        );
        extend(
            &mut self.quads,
            child.quads.iter().map(|polygon| polygon.map(shift)),
        );
    }
}

fn extend<T>(values: &mut Box<[T]>, added: impl Iterator<Item = T>) {
    let mut grown = std::mem::take(values).into_vec();
    grown.extend(added);
    *values = grown.into_boxed_slice();
}

/// Reads an attribute array; a missing or non-numeric component is zero, as in vanilla.
fn attribute<const N: usize>(
    value: Option<&Value>,
    skipped: &mut u32,
) -> Box<[[EntityGeometryScalar; N]]> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Box::default();
    };
    values
        .iter()
        .map(|entry| {
            std::array::from_fn(|axis| {
                let component = entry.get(axis).and_then(Value::as_f64).unwrap_or(0.0);
                EntityGeometryScalar::new(component as f32).unwrap_or_else(|| {
                    *skipped += 1;
                    EntityGeometryScalar::ZERO
                })
            })
        })
        .collect()
}

/// A missing index is zero, as in vanilla; a negative or oversized index loses its polygon.
fn polygon_index(value: Option<&Value>) -> Option<u32> {
    let Some(value) = value else {
        return Some(0);
    };
    let Some(number) = value.as_f64() else {
        return Some(0);
    };
    (number >= 0.0 && number <= f64::from(u32::MAX)).then_some(number as u32)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mesh(polys: Value) -> (Option<EntityGeometryPolyMesh>, u32) {
        EntityGeometryPolyMesh::parse(&json!({
            "positions": [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]],
            "normals": [[0, 0, -1], [0, 0, -1], [0, 0, -1], [0, 0, -1]],
            "uvs": [[0, 0], [1, 0], [1, 1], [0, 1]],
            "polys": polys,
        }))
    }

    #[test]
    fn implicit_lists_consume_one_of_each_attribute_per_corner() {
        let (quads, skipped) = mesh(json!("quad_list"));
        let quads = quads.unwrap();
        assert_eq!(skipped, 0);
        assert!(quads.triangles.is_empty());
        assert_eq!(quads.quads[0][3], [3, 3, 3]);
        // Vanilla only tests for `tri_list`; every other name reads as quads.
        assert_eq!(mesh(json!("not_a_mode")).0.unwrap().quads.len(), 1);
        // Four corners are not a whole number of triangles.
        assert_eq!(mesh(json!("tri_list")), (None, 1));
    }

    #[test]
    fn implicit_lists_require_matching_attribute_counts() {
        let (parsed, skipped) = EntityGeometryPolyMesh::parse(&json!({
            "positions": [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            "normals": [[0, 0, 1]], "uvs": [[0, 0], [1, 0], [0, 1]], "polys": "tri_list"
        }));
        assert_eq!(parsed, None);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn explicit_polygons_keep_independent_indices_and_lose_only_bad_neighbours() {
        let (parsed, skipped) = mesh(json!([
            [[0, 0, 2], [1, 1, 0], [2, 2, 1]],
            [[0, 0, 0], [99, 0, 1], [2, 0, 2], [3, 0, 3]],
            [[0, 0, 0], [-1, 0, 1], [2, 0, 2]],
            "not a polygon",
            [[0, 1, 2], [1, 2, 3], [2, 3, 0], [3, 0, 1]],
        ]));
        let parsed = parsed.unwrap();
        assert_eq!(skipped, 3);
        assert_eq!(parsed.triangles[..], [[[0, 0, 2], [1, 1, 0], [2, 2, 1]]]);
        assert_eq!(
            parsed.quads[..],
            [[[0, 1, 2], [1, 2, 3], [2, 3, 0], [3, 0, 1]]]
        );
    }

    #[test]
    fn explicit_polygons_of_other_sizes_read_four_corners_with_missing_indices_zero() {
        let (parsed, skipped) = mesh(json!([
            [[1, 1, 1], [2], [3, 3, 3], [3, 3, 3], [2, 2, 2]],
            [[1, 2, 3]],
        ]));
        let parsed = parsed.unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(
            parsed.quads[0],
            [[1, 1, 1], [2, 0, 0], [3, 3, 3], [3, 3, 3]]
        );
        assert_eq!(
            parsed.quads[1],
            [[1, 2, 3], [0, 0, 0], [0, 0, 0], [0, 0, 0]]
        );
    }

    #[test]
    fn malformed_attributes_read_as_zero_and_unusable_meshes_draw_nothing() {
        let (parsed, skipped) = EntityGeometryPolyMesh::parse(&json!({
            "positions": [[1], "x", [1e30, 0, 0]], "normals": [[0, "y", 1]], "uvs": [[0.5]],
            "polys": [[[0, 0, 0], [1, 0, 0], [2, 0, 0]]],
        }));
        let parsed = parsed.unwrap();
        assert_eq!(skipped, 1, "only the out-of-bound component is counted");
        assert_eq!(parsed.positions[0][0].get(), 1.0);
        assert_eq!(parsed.positions[1], [EntityGeometryScalar::ZERO; 3]);
        assert_eq!(parsed.positions[2], [EntityGeometryScalar::ZERO; 3]);
        assert!(!parsed.normalized_uvs);
        assert_eq!(EntityGeometryPolyMesh::parse(&json!(3)), (None, 1));
        assert_eq!(EntityGeometryPolyMesh::parse(&json!({})), (None, 0));
        assert_eq!(mesh(json!(7)), (None, 1));
    }

    #[test]
    fn appended_child_polygons_address_their_own_attributes() {
        let mut base = mesh(json!("quad_list")).0.unwrap();
        let mut child = EntityGeometryPolyMesh::parse(&json!({
            "normalized_uvs": true,
            "positions": [[5, 0, 0], [6, 0, 0], [6, 1, 0]],
            "normals": [[1, 0, 0]],
            "uvs": [[0.5, 0.5]],
            "polys": [[[0, 0, 0], [1, 0, 0], [2, 0, 0]]],
        }))
        .0
        .unwrap();
        base.append(&child);
        assert!(base.normalized_uvs);
        assert_eq!(base.quads.len(), 1);
        assert_eq!(base.triangles[0], [[4, 4, 4], [5, 4, 4], [6, 4, 4]]);
        assert_eq!(base.positions[6], child.positions[2]);
        assert!(base.validate().is_ok());
        child.triangles[0][2][0] = 3;
        assert!(child.validate().is_err());
    }
}
