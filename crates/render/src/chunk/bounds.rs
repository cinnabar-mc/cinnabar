//! Shared conservative extents for the CPU and GPU terrain visibility paths.
use bevy::{camera::primitives::Aabb, math::Vec3A};

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
pub(in crate::chunk) const FULL_BOUNDS: [[i32; 3]; 2] = [[0; 3], [SIDE; 3]];
// Round outward after adding the admitted component to a one-block model overhang.
const MODEL_PADDING: i32 = (1.0 + block_transform::random_offset::MAX_AXIS_OFFSET) as i32 + 1;
/// Conservative model extents including admitted displacement and waving.
pub(in crate::chunk) const MODEL_BOUNDS: [[i32; 3]; 2] =
    [[-MODEL_PADDING; 3], [SIDE + MODEL_PADDING; 3]];

/// Blocks every side of a section holding world material blocks grows by: the farthest a
/// material's displacement, outline or particles reach, rounded up.
const WORLD_MATERIAL_PADDING: i32 =
    mod_render::world_material::publication::MAX_REACH_BLOCKS as i32 + 1;
/// Extents of a section holding world material blocks: they may draw past it by
/// [`WORLD_MATERIAL_PADDING`], further than any model overhang.
pub(in crate::chunk) const WORLD_MATERIAL_BOUNDS: [[i32; 3]; 2] = [
    [-WORLD_MATERIAL_PADDING; 3],
    [SIDE + WORLD_MATERIAL_PADDING; 3],
];
const _: () = assert!(WORLD_MATERIAL_PADDING >= MODEL_PADDING);

/// Which conservative extents a section culls with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::chunk) enum BoundsKind {
    /// Cube faces only: the section itself.
    Full = 0,
    /// Model geometry overhanging the section ([`MODEL_BOUNDS`]).
    Models = 1,
    /// Geometry bound to a world material, whose effects reach further
    /// ([`WORLD_MATERIAL_BOUNDS`]).
    WorldMaterials = 2,
}

impl BoundsKind {
    /// Every kind, in index order.
    pub(in crate::chunk) const ALL: [Self; 3] = [Self::Full, Self::Models, Self::WorldMaterials];

    /// The kind of a section with model geometry or not, and with geometry bound to a world
    /// material or not.
    pub(in crate::chunk) const fn of(models: bool, world_materials: bool) -> Self {
        if world_materials {
            Self::WorldMaterials
        } else if models {
            Self::Models
        } else {
            Self::Full
        }
    }

    /// The extents of a section of this kind.
    pub(in crate::chunk) fn aabb(self) -> Aabb {
        let [low, high] = match self {
            Self::Full => FULL_BOUNDS,
            Self::Models => MODEL_BOUNDS,
            Self::WorldMaterials => WORLD_MATERIAL_BOUNDS,
        };
        let low = Vec3A::from_array(low.map(|value| value as f32));
        let high = Vec3A::from_array(high.map(|value| value as f32));
        Aabb {
            center: (low + high) * 0.5,
            half_extents: (high - low) * 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_with_world_material_blocks_cull_with_the_widest_reach() {
        let reach = mod_render::world_material::publication::MAX_REACH_BLOCKS;
        let bounds = BoundsKind::WorldMaterials.aabb();
        let full = BoundsKind::Full.aabb();
        assert_eq!(bounds.center, full.center);
        assert!(bounds.half_extents.min_element() >= full.half_extents.max_element() + reach);
    }
}
