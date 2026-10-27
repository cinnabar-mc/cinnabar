//! The local player's view as guests see it: where the eye sits and how its angles read.

/// Eye height of a standing player above their feet, in blocks: a guest subtracts it from the
/// gameplay frame's `eye` to find where a standing player's feet are. The client checks it
/// against the protocol's own value.
pub const STANDING_PLAYER_EYE_HEIGHT: f32 = 1.62;

/// Converts observed actor YXZ degrees (forward -Z, pitch up) to Bedrock view degrees.
pub fn bedrock_view_angles(yaw: f32, pitch: f32) -> [f32; 2] {
    [(180. - yaw).rem_euclid(360.), -pitch]
}
