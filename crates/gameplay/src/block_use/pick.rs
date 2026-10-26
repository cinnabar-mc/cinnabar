//! The geometric result retained between a presented frame and a build action.

/// A picked block keeps its intercept even when its state changes before the tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockPick {
    pub position: [i32; 3],
    pub face: u8,
    pub point: [f32; 3],
    pub indirect: bool,
    /// The overlay geometry selected instead of the host block, when present.
    pub overlay: Option<sim::OverlayHit>,
}

/// Frame picking preserves misses and actors as well as direct and indirect block hits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickResult {
    Miss,
    Block(BlockPick),
    Actor {
        identity: actor_animation::ActorLifetimeId,
        point: [f32; 3],
    },
}

impl PickResult {
    /// The finite intercept that bounds this observation's refreshed block ray.
    pub fn point(self) -> Option<[f32; 3]> {
        match self {
            Self::Miss => None,
            Self::Block(block) => Some(block.point),
            Self::Actor { point, .. } => Some(point),
        }
    }

    /// Applies the native float centre/point distance test to a newly picked result.
    pub fn trimmed(self, eye: [f32; 3], reach: f64) -> Self {
        let point = match self {
            Self::Miss => return self,
            Self::Block(block) => block.position.map(|axis| axis as f32 + 0.5),
            Self::Actor { point, .. } => point,
        };
        let [x, y, z] = std::array::from_fn(|axis| eye[axis] - point[axis]);
        let reach = reach as f32;
        if reach * reach < z * z + y * y + x * x {
            Self::Miss
        } else {
            self
        }
    }

    /// Trims the new clip before falling back to an original block or still-live actor hit.
    pub fn refreshed(
        self,
        block: Option<BlockPick>,
        eye: [f32; 3],
        reach: f64,
        actor_alive: bool,
    ) -> Self {
        let refreshed = block.map_or(Self::Miss, Self::Block).trimmed(eye, reach);
        if matches!(refreshed, Self::Block(_)) {
            return refreshed;
        }
        match self {
            Self::Block(_) => self,
            Self::Actor { .. } if actor_alive => self,
            Self::Actor { .. } | Self::Miss => Self::Miss,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Refresh trimming precedes fallback; the original intercept survives a failed refreshed clip.
    #[test]
    fn refresh_preserves_original_blocks_and_only_live_actors_on_a_miss() {
        let block = BlockPick {
            position: [0, 0, 0],
            face: 1,
            point: [0.5, 1.0, 0.5],
            indirect: false,
            overlay: None,
        };
        let original = PickResult::Block(block);
        let too_far = BlockPick {
            position: [10, 0, 0],
            ..block
        };
        assert_eq!(
            original.refreshed(Some(too_far), [0.5; 3], 5.7, false),
            original
        );
        assert_eq!(
            PickResult::Miss.refreshed(Some(too_far), [0.5; 3], 5.7, false),
            PickResult::Miss
        );
        let actor = PickResult::Actor {
            identity: actor_animation::ActorLifetimeId {
                session_id: 1,
                dimension: 0,
                runtime_id: 7,
                spawn_revision: 1,
            },
            point: [0.5, 1.0, 0.5],
        };
        assert_eq!(actor.refreshed(None, [0.5; 3], 5.7, true), actor);
        assert_eq!(
            actor.refreshed(None, [0.5; 3], 5.7, false),
            PickResult::Miss
        );
        assert_eq!(actor.refreshed(Some(block), [0.5; 3], 5.7, true), original);
    }
    /// A boundary subtraction rounds in float before its squared distance is compared.
    #[test]
    fn reach_trimming_uses_float_boundary_arithmetic() {
        let block = PickResult::Block(BlockPick {
            position: [5, 0, 0],
            face: 4,
            point: [5.0, 0.5, 0.5],
            indirect: false,
            overlay: None,
        });
        let eye = [f32::from_bits(0.5_f32.to_bits() - 1), 0.5, 0.5];
        assert_eq!(block.trimmed(eye, 5.0), block);
        assert_eq!(block.trimmed([0.49, 0.5, 0.5], 5.0), PickResult::Miss);
    }
}
