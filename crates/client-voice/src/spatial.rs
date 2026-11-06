//! Distance attenuation and stereo panning for a remote speaker.

use crate::proto::Position;

/// The local player's ears. `yaw_deg` follows Minecraft: 0 faces +Z (south), 90 faces -X (west).
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    pub pos: Position,
    pub yaw_deg: f32,
}

/// Per-channel gains to apply to a mono voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spatial {
    pub left: f32,
    pub right: f32,
}

/// Fraction of the range inside which a speaker is heard at full volume.
const FULL_VOLUME_FRACTION: f32 = 0.25;

/// Linear gain in `[0, 1]`: full volume up close, fading to silence at `range`.
pub fn distance_gain(distance: f32, range: f32) -> f32 {
    if !distance.is_finite() || range <= 0.0 || distance >= range {
        return 0.0;
    }
    let near = range * FULL_VOLUME_FRACTION;
    if distance <= near {
        1.0
    } else {
        (range - distance) / (range - near)
    }
}

/// Stereo gains for `speaker` as heard by `listener`, or `None` if inaudible
/// (different dimension or out of range).
pub fn spatialize(listener: &Listener, speaker: &Position, range: f32) -> Option<Spatial> {
    if listener.pos.dim != speaker.dim {
        return None;
    }
    let distance = listener.pos.distance(speaker);
    let gain = distance_gain(distance, range);
    if gain <= 0.0 {
        return None;
    }
    // Horizontal direction to the speaker, relative to where the listener faces.
    let (dx, dz) = (speaker.x - listener.pos.x, speaker.z - listener.pos.z);
    let horizontal = (dx * dx + dz * dz).sqrt();
    let pan = if horizontal < 0.5 {
        0.0 // practically on top of the listener: centered
    } else {
        let yaw = listener.yaw_deg.to_radians();
        // Facing +Z at yaw 0, the right-hand side is -X.
        let (rx, rz) = (-yaw.cos(), -yaw.sin());
        ((dx * rx + dz * rz) / horizontal).clamp(-1.0, 1.0)
    };
    // Equal-power pan law.
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    Some(Spatial {
        left: gain * angle.cos(),
        right: gain * angle.sin(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(yaw: f32) -> Listener {
        Listener {
            pos: Position::new(0, 0.0, 64.0, 0.0),
            yaw_deg: yaw,
        }
    }

    fn speaker(x: f32, z: f32) -> Position {
        Position::new(0, x, 64.0, z)
    }

    #[test]
    fn gain_curve_is_full_then_linear_then_silent() {
        assert_eq!(distance_gain(0.0, 48.0), 1.0);
        assert_eq!(distance_gain(12.0, 48.0), 1.0);
        assert!((distance_gain(30.0, 48.0) - 0.5).abs() < 1e-6);
        assert_eq!(distance_gain(48.0, 48.0), 0.0);
        assert_eq!(distance_gain(100.0, 48.0), 0.0);
        assert_eq!(distance_gain(f32::NAN, 48.0), 0.0);
    }

    #[test]
    fn other_dimension_or_far_is_inaudible() {
        let far = speaker(100.0, 0.0);
        assert!(spatialize(&listener(0.0), &far, 48.0).is_none());
        let nether = Position::new(1, 1.0, 64.0, 1.0);
        assert!(spatialize(&listener(0.0), &nether, 48.0).is_none());
    }

    #[test]
    fn speaker_straight_ahead_is_centered() {
        // Facing +Z at yaw 0, a speaker at +Z is dead ahead.
        let s = spatialize(&listener(0.0), &speaker(0.0, 5.0), 48.0).unwrap();
        assert!((s.left - s.right).abs() < 1e-5);
    }

    #[test]
    fn west_of_a_south_facing_listener_is_on_the_right() {
        let s = spatialize(&listener(0.0), &speaker(-5.0, 0.0), 48.0).unwrap();
        assert!(s.right > 0.99 && s.left < 0.01, "{s:?}");
    }

    #[test]
    fn east_of_a_south_facing_listener_is_on_the_left() {
        let s = spatialize(&listener(0.0), &speaker(5.0, 0.0), 48.0).unwrap();
        assert!(s.left > 0.99 && s.right < 0.01, "{s:?}");
    }

    #[test]
    fn turning_moves_the_sound_across_the_stereo_field() {
        // Facing west (yaw 90), north (-Z) is on the right.
        let s = spatialize(&listener(90.0), &speaker(0.0, -5.0), 48.0).unwrap();
        assert!(s.right > 0.99, "{s:?}");
    }

    #[test]
    fn power_is_preserved_across_the_pan() {
        for x in [-5.0, -2.0, 0.0, 2.0, 5.0] {
            let s = spatialize(&listener(0.0), &speaker(x, 3.0), 48.0).unwrap();
            let power = s.left * s.left + s.right * s.right;
            assert!((power - 1.0).abs() < 1e-4, "x={x} power={power}");
        }
    }
}
