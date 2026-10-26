//! Bounds for callback-scoped actor, combat and world observations.

/// Maximum bytes in a stable selected world or server identity.
pub const MAX_WORLD_IDENTITY_BYTES: usize = 256;

/// Bounded cosmetic actor and combat observations per sandbox callback.
pub const MAX_OBSERVED_ACTORS: usize = 16;
pub const MAX_OBSERVED_ACTOR_BOXES: usize = 8;
pub const MAX_OBSERVED_ACTOR_NAME_BYTES: usize = 128;
pub const MAX_OBSERVED_COMBAT_EVENTS: usize = 64;
/// Runtime IDs one `read-actor-lives` call may request.
pub const MAX_ACTOR_LIFE_READS: usize = 64;
/// Sequences one `read-combat-lives` call may request, covering the retained history.
pub const MAX_COMBAT_LIFE_READS: usize = MAX_OBSERVED_COMBAT_EVENTS;
pub const MAX_COMBAT_READ_EVENTS: usize = 4;
