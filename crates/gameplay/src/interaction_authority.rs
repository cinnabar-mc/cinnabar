//! Frozen interaction authority and shared range checks.
use crate::mining::{
    FrozenMiningFrame, FrozenMiningRay, FrozenMiningSelection, FrozenMiningTarget,
};
use protocol::PlayerInputMode;

/// Wall-clock millis a deferred press may wait for fresh evidence or a tick, whatever the frame
/// rate, without outliving a stalled simulation.
pub const MAX_PENDING_INTERACTION_MILLIS: u64 =
    10 * simulation_clock::TICK_DURATION.as_millis() as u64;

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenBlockObservation {
    pub frame: FrozenMiningFrame,
    pub ray: FrozenMiningRay,
    pub reach: f64,
    pub input_mode: PlayerInputMode,
    pub selection: FrozenMiningSelection,
    pub target: FrozenMiningTarget,
}

impl FrozenBlockObservation {
    /// Distance from this ray's origin to its observed world-space block hit.
    pub fn hit_distance(&self) -> f64 {
        crate::melee::block_pick_distance(self.ray.origin, self.target.hit_world)
    }
}

/// Whether a pick's `point` lies within `reach` of `position`, the camera actor's ticked
/// position without camera offsets. Vanilla compares squared distances in single precision.
pub fn pick_point_in_range(point: [f32; 3], position: [f32; 3], reach: f64) -> bool {
    let [x, y, z] = std::array::from_fn(|axis| position[axis] - point[axis]);
    let reach = reach as f32;
    reach * reach >= z * z + y * y + x * x
}

/// Whether a picked block stays targeted. Vanilla measures to the block's centre, not to
/// the ray's intercept, so a hit inside the clipped ray can still be dropped.
pub fn block_pick_in_range(block: [i32; 3], position: [f32; 3], reach: f64) -> bool {
    pick_point_in_range(block.map(|axis| axis as f32 + 0.5), position, reach)
}

#[cfg(test)]
impl FrozenBlockObservation {
    /// A top-face hit on `position` holding `item` in slot 2.
    pub fn fixture(position: [i32; 3], face: u8, item: protocol::VerifiedNetworkItemStack) -> Self {
        let identity = sim::CollisionQuery::synthetic(()).identity;
        let relative_hit = [0.5, 1.0, 0.5];
        Self {
            frame: FrozenMiningFrame {
                session_generation: simulation_clock::identity::SessionGeneration::new(7),
                position_authority_generation:
                    simulation_clock::identity::PositionAuthorityGeneration::new(0),
                input_authority_generation:
                    semantic_input::identity::InputAuthorityGeneration::INITIAL,
                input_frame_sequence: semantic_input::identity::InputSequence::new(1),
                fifo_sequence: protocol::identity::FifoSequence::new(1),
                physics_tick: simulation_clock::PhysicsTick::new(101),
                pose_generation: render_api::PoseGeneration::new(1),
            },
            ray: FrozenMiningRay {
                origin: [0.5, 65.62, 0.5],
                direction: [0.0, -1.0, 0.0],
                movement_world_identity: identity.clone(),
                world_identity: identity.clone(),
            },
            reach: 5.7,
            input_mode: PlayerInputMode::Mouse,
            selection: FrozenMiningSelection { slot: 2, item },
            target: FrozenMiningTarget {
                position,
                face,
                relative_hit,
                hit_world: std::array::from_fn(|axis| position[axis] as f32 + relative_hit[axis]),
                runtime_id: 9,
                identity,
            },
        }
    }
}

#[cfg(test)]
mod pick_range_tests {
    use super::*;

    #[test]
    fn block_trim_rounds_offsets_before_comparing_the_boundary() {
        let position = [0.5_f32.next_down(), 0.5, 0.5];
        assert!(block_pick_in_range([5, 0, 0], position, 5.0));
        assert!(!block_pick_in_range([5, 0, 0], [0.49, 0.5, 0.5], 5.0));
    }

    #[test]
    fn touch_reach_has_no_tighter_corner_limit() {
        // Centre 6.61 blocks away, minimum corner 7.46: only the centre is compared.
        let reach = crate::mining::survival_reach(PlayerInputMode::Touch);
        assert!(block_pick_in_range([-5, 62, -3], [0.5, 65.62, 0.5], reach));
    }

    #[test]
    fn diagonal_and_upward_hits_inside_the_ray_are_dropped_by_their_centre() {
        let reach = crate::mining::survival_reach(PlayerInputMode::Mouse);
        for (block, position, hit) in [
            ([8, 2, 4], [4.1, 2.5, 8.9], [8.0, 2.5, 5.0]),
            ([8, 7, 8], [4.9, 2.5, 8.9], [8.14, 7.0, 8.54]),
        ] {
            assert!(crate::melee::block_pick_distance(position, hit) < reach);
            assert!(!block_pick_in_range(block, position, reach));
        }
    }
}
