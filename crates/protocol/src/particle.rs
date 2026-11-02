//! Particle-bearing packets: `SpawnParticleEffect` and particle `LevelEvent`s.

use std::sync::Arc;

use bedrock_protocol_1_26_60::packets::{LevelEvent, SpawnParticleEffect};
use protocol_ids::LevelEvent as LevelEventId;

/// Longest effect identifier accepted from the wire.
pub const MAX_PARTICLE_NAME_BYTES: usize = 256;
/// Largest Molang variable map accepted from the wire.
pub const MAX_PARTICLE_VARIABLES_BYTES: usize = 16 * 1024;

/// Level events at or above this bit carry a legacy particle type in the low bits.
const LEVEL_EVENT_PARTICLE_FLAG: i32 = 0x4000;

#[derive(Debug, Clone, PartialEq)]
pub enum ParticleEvent {
    Level(LevelParticleEvent),
    Spawn(SpawnParticleEffectEvent),
    /// A critical or magic-critical hit animation on an actor.
    ActorCritical {
        actor_runtime_id: u64,
        magic: bool,
        /// The Animate packet's data, which vanilla binds as `variable.particle_count`.
        particle_count: f32,
    },
}

/// A level event that may present particles; the id is classified by the consumer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelParticleEvent {
    pub event_id: i32,
    pub position: [f32; 3],
    pub data: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpawnParticleEffectEvent {
    pub dimension: u8,
    /// Entity the position is relative to; `None` for world-space.
    pub actor_unique_id: Option<i64>,
    pub position: [f32; 3],
    pub effect: Arc<str>,
    /// Raw JSON Molang variable map, if the server sent one.
    pub molang_variables: Option<Arc<str>>,
}

fn is_particle_level_event(event_id: i32) -> bool {
    (2000..=2040).contains(&event_id)
        || (LevelEventId::ParticlesCrackBlockDown.wire_value()
            ..=LevelEventId::ParticlesCrackBlockEast.wire_value())
            .contains(&event_id)
        || (3609..=3614).contains(&event_id)
        || event_id & LEVEL_EVENT_PARTICLE_FLAG != 0
}

/// Returns the event for particle-bearing level events; other ids and non-finite positions
/// are skipped.
pub(crate) fn normalize_level_event(packet: &LevelEvent) -> Option<ParticleEvent> {
    if !is_particle_level_event(packet.event_id.0) {
        return None;
    }
    let position = packet.position.to_array();
    position.iter().all(|c| c.is_finite()).then_some(())?;
    Some(ParticleEvent::Level(LevelParticleEvent {
        event_id: packet.event_id.0,
        position,
        data: packet.data.0,
    }))
}

/// Returns the spawn event; an empty or oversized name, oversized variable map or
/// non-finite position skips the packet.
pub(crate) fn normalize_spawn(packet: SpawnParticleEffect) -> Option<ParticleEvent> {
    let position = packet.position.to_array();
    let effect_name = packet.effect_name.to_string_lossy();
    if !position.iter().all(|c| c.is_finite())
        || effect_name.is_empty()
        || effect_name.len() > MAX_PARTICLE_NAME_BYTES
    {
        return None;
    }
    let molang_variables = match packet.molang_variables {
        Some(json) if json.len() > MAX_PARTICLE_VARIABLES_BYTES => return None,
        Some(json) if !json.is_empty() => Some(Arc::from(json.to_string_lossy())),
        _ => None,
    };
    let actor = packet.actor_id.0;
    Some(ParticleEvent::Spawn(SpawnParticleEffectEvent {
        dimension: packet.dimension_id.0,
        actor_unique_id: (actor != -1).then_some(actor),
        position,
        effect: Arc::from(effect_name),
        molang_variables,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_60::wire;

    fn level(event_id: i32, x: f32) -> LevelEvent {
        LevelEvent {
            event_id: wire::ZigZag32(event_id),
            position: [x, 1.0, 2.0].into(),
            data: wire::ZigZag32(7),
        }
    }

    #[test]
    fn particle_level_events_pass_and_others_are_skipped() {
        assert!(normalize_level_event(&level(2001, 0.0)).is_some());
        assert!(normalize_level_event(&level(0x4000 | 8, 0.0)).is_some());
        assert!(normalize_level_event(&level(3001, 0.0)).is_none());
        assert!(normalize_level_event(&level(1000, 0.0)).is_none());
    }

    #[test]
    fn face_crack_packets_preserve_the_full_block_id() {
        for event_id in LevelEventId::ParticlesCrackBlockDown.wire_value()
            ..=LevelEventId::ParticlesCrackBlockEast.wire_value()
        {
            let mut packet = level(event_id, 0.5);
            packet.data = wire::ZigZag32(0xfabc_def0_u32 as i32);
            assert_eq!(
                normalize_level_event(&packet),
                Some(ParticleEvent::Level(LevelParticleEvent {
                    event_id,
                    position: [0.5, 1.0, 2.0],
                    data: packet.data.0,
                }))
            );
        }
    }

    #[test]
    fn non_finite_positions_are_skipped_not_fatal() {
        assert!(normalize_level_event(&level(2001, f32::NAN)).is_none());
    }
}
