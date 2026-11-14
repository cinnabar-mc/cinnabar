//! The packets the client sends for a tick: sprint and sneak changes, then position and look.

use super::{Abilities, Player};

/// Entity action ids for sneak and sprint changes.
const ACTION_START_SNEAKING: i32 = 1;
const ACTION_STOP_SNEAKING: i32 = 2;
const ACTION_START_SPRINTING: i32 = 4;
const ACTION_STOP_SPRINTING: i32 = 5;
/// Squared distance the player must move before its position is sent again.
const POSITION_THRESHOLD_SQUARED: f64 = 9.0e-4;
/// Ticks after which the position is sent even without movement.
const POSITION_REFRESH_TICKS: i32 = 20;

/// A movement packet in the form the 1.7.10 client sends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Packet {
    /// Entity action: start or stop sneaking or sprinting.
    Action(i32),
    /// Neither position nor look changed.
    Ground { on_ground: bool },
    /// Position: `y` is the feet and `stance` the game's `posY`.
    Position {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        on_ground: bool,
    },
    Look {
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    PositionLook {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    },
    /// The client's abilities after it toggled flight.
    Abilities {
        invulnerable: bool,
        flying: bool,
        allow_flying: bool,
        creative: bool,
        fly_speed: f32,
        walk_speed: f32,
    },
}

impl Packet {
    /// The abilities packet for the player's current abilities.
    pub(super) fn abilities(abilities: &Abilities) -> Self {
        Self::Abilities {
            invulnerable: abilities.invulnerable,
            flying: abilities.flying,
            allow_flying: abilities.allow_flying,
            creative: abilities.creative,
            fly_speed: abilities.fly_speed,
            walk_speed: abilities.walk_speed,
        }
    }
}

impl Player {
    /// Appends this tick's sprint and sneak changes and one position or look packet. Position is
    /// sent after moving more than 0.03 blocks from the last reported position, or every 20
    /// ticks; look is sent when either angle changed at all.
    pub(super) fn report_motion(&mut self, packets: &mut Vec<Packet>) {
        if self.sprinting != self.reported.sprinting {
            packets.push(Packet::Action(if self.sprinting {
                ACTION_START_SPRINTING
            } else {
                ACTION_STOP_SPRINTING
            }));
            self.reported.sprinting = self.sprinting;
        }
        let sneaking = self.keys.sneak;
        if sneaking != self.reported.sneaking {
            packets.push(Packet::Action(if sneaking {
                ACTION_START_SNEAKING
            } else {
                ACTION_STOP_SNEAKING
            }));
            self.reported.sneaking = sneaking;
        }
        let moved_x = self.x - self.reported.x;
        let moved_y = self.bounds.min_y - self.reported.min_y;
        let moved_z = self.z - self.reported.z;
        let turned_yaw = f64::from(self.yaw - self.reported.yaw);
        let turned_pitch = f64::from(self.pitch - self.reported.pitch);
        let moved = moved_x * moved_x + moved_y * moved_y + moved_z * moved_z
            > POSITION_THRESHOLD_SQUARED
            || self.reported.ticks_since_position >= POSITION_REFRESH_TICKS;
        let turned = turned_yaw != 0.0 || turned_pitch != 0.0;
        let on_ground = self.on_ground;
        let (x, y, stance, z) = (self.x, self.bounds.min_y, self.y, self.z);
        let (yaw, pitch) = (self.yaw, self.pitch);
        packets.push(match (moved, turned) {
            (true, true) => Packet::PositionLook {
                x,
                y,
                stance,
                z,
                yaw,
                pitch,
                on_ground,
            },
            (true, false) => Packet::Position {
                x,
                y,
                stance,
                z,
                on_ground,
            },
            (false, true) => Packet::Look {
                yaw,
                pitch,
                on_ground,
            },
            (false, false) => Packet::Ground { on_ground },
        });
        self.reported.ticks_since_position += 1;
        if moved {
            self.reported.x = self.x;
            self.reported.min_y = self.bounds.min_y;
            self.reported.z = self.z;
            self.reported.ticks_since_position = 0;
        }
        if turned {
            self.reported.yaw = self.yaw;
            self.reported.pitch = self.pitch;
        }
    }
}
