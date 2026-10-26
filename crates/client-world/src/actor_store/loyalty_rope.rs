//! The rope a thrown trident draws back to its owner, which vanilla's native renderer adds
//! outside the trident's JSON rig.

use assets::facts::entity::NativeEntityClass;
use protocol_ids::{ActorDataKey, ActorFlag};

use super::{ActorSnapshot, ActorStore, entities};

/// Vanilla lowers the rope's owner end this far below the owner's actor position.
const OWNER_END_DROP: f32 = 0.5;
/// Fraction of the owner's tick yaw change vanilla applies across a full frame.
const OWNER_YAW_STEP: f32 = 0.5;
/// Coil phase per tick, in radians, kept in double precision as vanilla computes it.
const PHASE_PER_TICK: f64 = -0.1;

/// Loyalty rope inputs for one thrown trident, in world blocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoyaltyRopeView {
    pub runtime_id: u64,
    /// Interpolated trident position, where the rope starts.
    pub trident: [f32; 3],
    /// Offset from the trident to the rope's owner end.
    pub span: [f32; 3],
    pub owner_yaw_degrees: f32,
    /// Coil phase in radians at the trident end.
    pub phase: f32,
    /// Ticks the trident has been returning to its owner.
    pub return_ticks: u32,
}

impl ActorSnapshot {
    /// Counts a thrown trident's ticks with the return flag set, as vanilla's normal tick does.
    pub(super) fn advance_loyalty_return(&mut self) {
        if self.entity_fact.native_class == NativeEntityClass::ThrownTrident
            && self.flag(ActorFlag::ReturnTrident.wire_value())
        {
            self.status.loyalty_return_ticks = self.status.loyalty_return_ticks.saturating_add(1);
        }
    }
}

impl ActorStore {
    /// Publishes a rope for every thrown trident whose owner is present, as vanilla draws one
    /// whenever the owner resolves; it stays a one-segment stub until the trident returns.
    pub(crate) fn loyalty_ropes(&self, partial_tick: f32) -> Vec<LoyaltyRopeView> {
        let alpha = partial_tick.clamp(0.0, 1.0);
        let rest = 1.0 - alpha;
        self.effect_members
            .tridents
            .iter()
            .filter_map(|runtime| {
                let trident = self.actors.get(runtime)?;
                let owner = entities::metadata_i64(trident, ActorDataKey::Owner.wire_value())
                    .and_then(|id| self.actor_by_unique(id))?;
                let owner_now = owner.native_position();
                let owner_previous = owner
                    .previous_pose
                    .native_position(owner.network_position_offset());
                let trident_now = trident.native_position();
                // Vanilla blends the trident's current position with the owner's previous
                // one, so the span shrinks toward the trident early in each tick.
                let span = std::array::from_fn(|axis| {
                    let drop = if axis == 1 { OWNER_END_DROP } else { 0.0 };
                    ((owner_now[axis] - drop) * alpha + (owner_previous[axis] - drop) * rest)
                        - (trident_now[axis] * alpha + owner_previous[axis] * rest)
                });
                let previous_yaw = owner.previous_pose.yaw;
                let owner_yaw_degrees =
                    (owner.yaw - previous_yaw) * OWNER_YAW_STEP * alpha + previous_yaw;
                // Vanilla wraps the unique id and tick count as 32-bit unsigned values.
                let ticks = (trident.unique_id as u32).wrapping_add(trident.status.age_ticks)
                    as f32
                    + alpha;
                Some(LoyaltyRopeView {
                    runtime_id: trident.runtime_id,
                    trident: trident.interpolated_position(alpha)?,
                    span,
                    owner_yaw_degrees,
                    phase: (f64::from(ticks) * PHASE_PER_TICK) as f32,
                    return_ticks: trident.status.loyalty_return_ticks,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "loyalty_rope_tests.rs"]
mod tests;
