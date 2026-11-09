//! Fishing-hook state that the hook's event effects read when the event arrives.

use assets::facts::entity::NativeEntityClass;
use protocol::{ActorMetadataValue, ActorStatusKind};
use protocol_ids::{ActorDataKey, ActorFlag};

use super::ActorSnapshot;

/// A fishing hook's state at the moment one of its fishing events was applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FishingHookPose {
    /// Bottom of the hook's collision box.
    pub bottom: f32,
    /// Width of the hook's collision box.
    pub width: f32,
    /// Synced offset of the approaching fish from the hook, on x and z.
    pub fish_offset: [f32; 2],
    /// Synced approach angle of the fish, in radians.
    pub fish_angle: f32,
    /// The hook's silent flag, which mutes its sounds.
    pub silent: bool,
}

impl FishingHookPose {
    /// Captures a hook's state for a fishing event; `None` for other kinds and actors, which
    /// ignore these event ids.
    pub(super) fn for_event(actor: &ActorSnapshot, kind: ActorStatusKind) -> Option<Self> {
        if !matches!(
            kind,
            ActorStatusKind::FishHookPosition
                | ActorStatusKind::FishHookBite
                | ActorStatusKind::FishHookTease
        ) || actor.entity_fact.native_class != NativeEntityClass::FishingHook
        {
            return None;
        }
        let (min, max) = actor.bounding_box()?;
        Some(Self {
            bottom: min[1],
            width: max[0] - min[0],
            fish_offset: [
                synced_float(actor, ActorDataKey::FishX),
                synced_float(actor, ActorDataKey::FishZ),
            ],
            fish_angle: synced_float(actor, ActorDataKey::FishAngle),
            silent: actor.flag(ActorFlag::Silent.wire_value()),
        })
    }
}

/// A synced float, reading 0 when the server has not sent it as a float.
fn synced_float(actor: &ActorSnapshot, key: ActorDataKey) -> f32 {
    match actor.metadata.get(&key.wire_value()) {
        Some(ActorMetadataValue::Float(value)) => *value,
        _ => 0.0,
    }
}

#[cfg(test)]
#[path = "fishing_tests.rs"]
mod tests;
