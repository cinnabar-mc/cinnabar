//! Shared Bedrock 1.12 combat presentation rules.

/// Bedrock 1.12 rules shared by camera, swing sampling and shader uploads.
#[derive(Clone, Copy, Debug)]
pub struct Bedrock112Rules {
    pub hurt_overlay: [f32; 4],
    pub hurt_ticks: u8,
    pub glint_period_ms: [i32; 2],
    pub glint_rotation_radians: [f32; 2],
    pub walk_gain: f32,
    pub bob_cap: f32,
    pub bob_follow: f32,
    pub horizontal_gain: f32,
    pub roll_radians: f32,
    pub pitch_radians: f32,
    pub pitch_phase: f32,
    pub hurt_radians: f32,
    pub death_numerator_radians: f32,
    pub death_offset_radians: f32,
    pub death_tick_offset: f32,
    pub sine_step: f32,
    pub sine_index_scale: f32,
    pub swing_body_radians: f32,
    pub swing_strike_radians: f32,
    pub swing_head_gain: f32,
    pub swing_head_offset: f32,
    pub swing_main_yaw_gain: f32,
    pub swing_shoulder_pixels: f32,
    pub arm_swing_translation: [f32; 3],
    pub item_swing_translation: [f32; 3],
    pub arm_anchor: [f32; 3],
    pub item_anchor: [f32; 3],
    pub equip_translation: f32,
    pub arm_base_yaw: f32,
    pub arm_swing_yaw: f32,
    pub arm_swing_roll: f32,
    pub arm_grip_translation: [f32; 3],
    pub arm_grip_radians: [f32; 3],
    pub arm_model_translation: [f32; 3],
    /// Y, Z, then X turns of the main-hand item.
    pub item_swing_radians: [f32; 3],
    pub item_scale: f32,
}

/// Bedrock 1.12 rules. The profile changes presentation only.
pub const BEDROCK_112: Bedrock112Rules = Bedrock112Rules {
    hurt_overlay: [1.0, 0.0, 0.0, 0.6],
    hurt_ticks: 10,
    glint_period_ms: [1750, 3000],
    glint_rotation_radians: [
        -20.0 * std::f32::consts::PI / 180.0,
        80.0 * std::f32::consts::PI / 180.0,
    ],
    walk_gain: 0.6,
    bob_cap: 0.1,
    bob_follow: 0.4,
    horizontal_gain: 0.65,
    roll_radians: 0.052_359_88,
    pitch_radians: 0.087_266_46,
    pitch_phase: -0.2,
    hurt_radians: -0.244_346_1,
    death_numerator_radians: -139.626_34,
    death_offset_radians: 0.698_131_7,
    death_tick_offset: 200.0,
    sine_step: f32::from_bits(0x38c9_0fdb),
    sine_index_scale: 10_430.378,
    swing_body_radians: 0.2,
    swing_strike_radians: 1.2,
    swing_head_gain: 0.75,
    swing_head_offset: 0.7,
    swing_main_yaw_gain: 3.0,
    swing_shoulder_pixels: 5.0,
    arm_swing_translation: [-0.3, 0.4, -0.4],
    item_swing_translation: [-0.4, 0.2, -0.2],
    arm_anchor: [0.64, -1.2, -0.72],
    item_anchor: [0.56, -1.12, -0.72],
    equip_translation: 0.6,
    arm_base_yaw: 45.0 * std::f32::consts::PI / 180.0,
    arm_swing_yaw: 70.0 * std::f32::consts::PI / 180.0,
    arm_swing_roll: -20.0 * std::f32::consts::PI / 180.0,
    arm_grip_translation: [-1.0, 3.6, 3.5],
    arm_grip_radians: [
        120.0 * std::f32::consts::PI / 180.0,
        200.0 * std::f32::consts::PI / 180.0,
        -135.0 * std::f32::consts::PI / 180.0,
    ],
    arm_model_translation: [5.6, 0.0, 0.0],
    item_swing_radians: [
        -20.0 * std::f32::consts::PI / 180.0,
        -20.0 * std::f32::consts::PI / 180.0,
        -80.0 * std::f32::consts::PI / 180.0,
    ],
    item_scale: 0.4,
};

/// Selects the explicit combat profile independently of Java pose selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombatVisuals {
    Modern,
    Legacy112,
}

/// One code-level choice for all consumers; Java poses and items take precedence.
pub const DEFAULT_COMBAT_VISUALS: CombatVisuals = CombatVisuals::Legacy112;

impl Default for CombatVisuals {
    /// Follows the shared code-level profile without a saved user preference.
    fn default() -> Self {
        DEFAULT_COMBAT_VISUALS
    }
}
