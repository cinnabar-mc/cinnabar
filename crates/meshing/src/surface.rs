//! Declarative, visual-only coverage for axis-aligned model faces.
//!
//! Rules use network identities already resolved by the caller. They never alter world
//! storage, selection shapes, collision, or the separate destruction-overlay geometry.

use crate::Face;
use world::MeshNeighbourhood;

/// Packed draw ABI: low bits select one of the template's at most 32 quads.
pub const QUAD_INDEX_MASK: u32 = 31;
pub const TILE_SHIFT: u32 = 5;
pub const TILE_MASK: u32 = 15;
/// Tiles in the three-by-three face partition.
pub const TILE_COUNT: u32 = 9;
pub const BORDER_SHIFT: u32 = 9;
pub const DRAW_WORD_MASK: u32 = 65535;
const _: () = assert!(assets::MAX_MODEL_TEMPLATE_QUADS <= QUAD_INDEX_MASK as usize + 1);

/// A three-by-three face partition, with independently visible corners, edges and centre.
/// Bits run along the face's positive U axis, then its positive V axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub tiles: u16,
    /// Width of each perimeter strip in 1/256-block units, in 1..=127.
    pub border: u8,
}

/// One relative neighbour test. Missing terrain never satisfies a test, even a negative one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeighbourCondition {
    /// Offset along face U, face V and the outward normal, each in -1..=1.
    pub offset: [i8; 3],
    pub present: bool,
}

/// Adds coverage when all relative neighbour tests hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageClause {
    pub conditions: Vec<NeighbourCondition>,
    pub tiles: u16,
}

/// A caller-resolved selector and its face-local coverage policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRule {
    /// Sorted network identities; members share a connection group.
    pub states: Vec<u32>,
    pub border: u8,
    pub clauses: Vec<CoverageClause>,
}

impl SurfaceRule {
    /// Rejects malformed partitions and offsets before admitting retained rules.
    pub fn validate(&self) -> bool {
        !self.states.is_empty()
            && self.states.windows(2).all(|pair| pair[0] < pair[1])
            && (1..=127).contains(&self.border)
            && self.clauses.iter().all(|clause| {
                clause.tiles & !0x1ff == 0
                    && clause.conditions.iter().all(|condition| {
                        condition.offset != [0; 3]
                            && condition
                                .offset
                                .iter()
                                .all(|value| (-1..=1).contains(value))
                    })
            })
    }

    /// Resolves coverage without changing the underlying block or texture.
    /// Unknown neighbours conservatively keep the entire face until terrain arrives.
    pub fn coverage(
        &self,
        neighbourhood: &MeshNeighbourhood<'_>,
        coordinate: [i32; 3],
        face: Face,
        air_network_id: u32,
    ) -> Coverage {
        let mut tiles = 0;
        for clause in &self.clauses {
            let mut matches = true;
            for condition in &clause.conditions {
                let offset = face_offset(face, condition.offset);
                let position = std::array::from_fn(|axis| coordinate[axis] + offset[axis]);
                let Some((section, [x, y, z])) = neighbourhood.block_source(position) else {
                    return Coverage {
                        tiles: 0x1ff,
                        border: self.border,
                    };
                };
                let value = section.runtime_id(0, x, y, z).unwrap_or(air_network_id);
                matches &= self.states.binary_search(&value).is_ok() == condition.present;
            }
            if matches {
                tiles |= clause.tiles;
            }
        }
        Coverage {
            tiles,
            border: self.border,
        }
    }
}

/// Converts face-local offsets to block coordinates using the mesher's positive U/V axes.
fn face_offset(face: Face, [u, v, n]: [i8; 3]) -> [i32; 3] {
    let [u, v, n] = [u, v, n].map(i32::from);
    match face {
        Face::NegativeX => [-n, v, u],
        Face::PositiveX => [n, v, u],
        Face::NegativeY => [u, -n, v],
        Face::PositiveY => [u, n, v],
        Face::NegativeZ => [u, v, -n],
        Face::PositiveZ => [u, v, n],
    }
}

/// Accepts only complete, untransformed axis-aligned cell faces.
pub(crate) fn full_face(quad: &assets::ModelQuad) -> Option<Face> {
    let face = match quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK {
        1 => Face::NegativeY,
        2 => Face::PositiveY,
        3 => Face::NegativeX,
        4 => Face::PositiveX,
        5 => Face::NegativeZ,
        6 => Face::PositiveZ,
        _ => return None,
    };
    let normal = face_offset(face, [0, 0, 1]);
    let axis = normal.iter().position(|value| *value != 0)?;
    let plane = if normal[axis] > 0 { 256 } else { 0 };
    let mut seen = 0u8;
    for position in quad.positions {
        if position[axis] != plane {
            return None;
        }
        let axes = match axis {
            0 => [1, 2],
            1 => [0, 2],
            _ => [0, 1],
        };
        let [u, v] = [position[axes[0]], position[axes[1]]];
        if ![0, 256].contains(&u) || ![0, 256].contains(&v) {
            return None;
        }
        seen |= 1 << ((u / 256) + 2 * (v / 256));
    }
    (seen == 15).then_some(face)
}

/// Emits bounded face patches using the original model and texture table.
pub(crate) fn push_draws(
    output: &mut Vec<crate::PackedModelDrawRef>,
    draw: crate::PackedModelDrawRef,
    coverage: Option<Coverage>,
) {
    if let Some(coverage) = coverage {
        for tile in 0..9 {
            if coverage.tiles & (1 << tile) != 0 {
                output.push(draw.with_surface_tile(tile, coverage.border));
            }
        }
    } else {
        output.push(draw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_and_partitions_are_validated() {
        let mut rule = SurfaceRule {
            states: vec![2, 4],
            border: 16,
            clauses: vec![],
        };
        assert!(rule.validate());
        rule.states.reverse();
        assert!(!rule.validate());
        rule.states.reverse();
        rule.border = 128;
        assert!(!rule.validate());
        rule.border = 16;
        rule.clauses.push(CoverageClause {
            conditions: vec![],
            tiles: 1 << 9,
        });
        assert!(!rule.validate());
    }

    #[test]
    fn face_axes_preserve_diagonals_and_outward_normals() {
        for (face, normal) in [
            (Face::NegativeX, [-1, 0, 0]),
            (Face::PositiveX, [1, 0, 0]),
            (Face::NegativeY, [0, -1, 0]),
            (Face::PositiveY, [0, 1, 0]),
            (Face::NegativeZ, [0, 0, -1]),
            (Face::PositiveZ, [0, 0, 1]),
        ] {
            assert_eq!(face_offset(face, [0, 0, 1]), normal);
            let u = face_offset(face, [1, 0, 0]);
            let v = face_offset(face, [0, 1, 0]);
            assert_eq!(
                face_offset(face, [1, -1, 0]),
                std::array::from_fn(|i| u[i] - v[i])
            );
        }
    }
}
