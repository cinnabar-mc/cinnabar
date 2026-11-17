//! A Java Edition server's snapshot of the player's movement speed attribute.

use serde::{Deserialize, Serialize};

/// Most modifiers a snapshot carries; a server that sends more has the rest left out.
pub const MAX_JAVA_SPEED_MODIFIERS: usize = 8;

/// One attribute modifier as the server sent it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JavaSpeedModifier {
    /// The modifier's UUID.
    pub id: u128,
    pub amount: f64,
    /// The 1.7.10 operation: 0 adds, 1 multiplies the base, 2 multiplies the total.
    pub operation: u8,
}

/// The server's last `generic.movementSpeed` snapshot. Each snapshot replaces the player's
/// attribute once, on the tick it arrives, as the 1.7.10 client applies it: a new `sequence`
/// marks a new snapshot even when its values repeat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JavaSpeedSnapshot {
    pub sequence: u32,
    pub base: f64,
    pub modifiers: [JavaSpeedModifier; MAX_JAVA_SPEED_MODIFIERS],
    pub modifier_count: u8,
}

impl JavaSpeedSnapshot {
    /// The modifiers the snapshot holds.
    #[must_use]
    pub fn modifiers(&self) -> &[JavaSpeedModifier] {
        &self.modifiers[..usize::from(self.modifier_count).min(MAX_JAVA_SPEED_MODIFIERS)]
    }
}

/// Most recent explosion pushes an input carries.
pub const MAX_JAVA_PUSHES: usize = 8;

/// One explosion's push of the player, which 1.7.10 adds to its motion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JavaPush {
    /// Arrival order; a Java tick applies each push it has not seen once.
    pub sequence: u32,
    pub motion: [f64; 3],
}

/// The server's recent explosion pushes, oldest first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct JavaPushes {
    pub pushes: [JavaPush; MAX_JAVA_PUSHES],
    pub count: u8,
}

impl JavaPushes {
    /// The pushes held, oldest first.
    #[must_use]
    pub fn pushes(&self) -> &[JavaPush] {
        &self.pushes[..usize::from(self.count).min(MAX_JAVA_PUSHES)]
    }
}
