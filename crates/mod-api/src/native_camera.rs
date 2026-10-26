//! Local camera effects shared by native packet callbacks and the client adapter.

use serde::{Deserialize, Serialize};

/// The optional `camera-frame` export receives the same bounded frame observations.
pub const CAPABILITY: u32 = 4;
/// Optional retained look and walking intents on the camera-frame transport.
pub const INPUT_INTENTS: u32 = 64;
/// Maximum presented view velocity accepted by `Effects::view`, in blocks per real second.
pub const MAX_VIEW_SPEED: f32 = 100.0;

/// An absolute presented camera pose computed by the guest for the frame its callback
/// observed, in world blocks and actor YXZ radians. Between callbacks the host only advances
/// `position` by `velocity`, for the real time since that frame was sent.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewPose {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Blocks per real second.
    pub velocity: [f32; 3],
}

impl ViewPose {
    /// Accepts finite world coordinates, a bounded pitch and a bounded speed.
    pub fn is_valid(self) -> bool {
        self.position
            .iter()
            .all(|value| value.is_finite() && value.abs() <= crate::MAX_PRIMITIVE_COORDINATE)
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.pitch.abs() <= std::f32::consts::FRAC_PI_2
            && self.velocity.iter().all(|value| value.is_finite())
            && self.velocity[0]
                .hypot(self.velocity[1])
                .hypot(self.velocity[2])
                <= MAX_VIEW_SPEED
    }
}

/// Current-frame degrees added to the first-person view where the native damage shake applies,
/// as the WIT `camera.view-tilt`; `suppress_hurt_shake` removes that shake.
///
/// Positive `pitch` turns the view up. Positive `roll` turns it the way a native shake without
/// direction does: the view's right edge rises, so the world appears to turn clockwise. A native
/// shake of angle `a` from hurt direction `d` is approximately roll `a cos d` and pitch
/// `-a sin d`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewTilt {
    pub roll: f32,
    pub pitch: f32,
    pub suppress_hurt_shake: bool,
}

impl ViewTilt {
    /// Accepts finite angles within `MAX_VIEW_TILT_DEGREES`.
    pub fn is_valid(self) -> bool {
        [self.roll, self.pitch]
            .iter()
            .all(|value| value.is_finite() && value.abs() <= crate::MAX_VIEW_TILT_DEGREES)
    }
}

/// Absolute view intent, evaluated against the current pose on every host frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookIntent {
    pub yaw: f32,
    pub pitch: f32,
    /// Radians per second; independent of callback delivery frequency.
    pub speed: f32,
}

impl LookIntent {
    /// Rejects non-finite headings and unbounded turning rates.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.pitch.abs() <= std::f32::consts::FRAC_PI_2
            && self.speed.is_finite()
            && (0.0..=20.0).contains(&self.speed)
    }
}

/// Ordinary walking controls in a supplied yaw frame, before client slowdown and physics.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementIntent {
    pub yaw: f32,
    /// Positive X is right, positive Y is forward.
    pub axes: [f32; 2],
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

impl MovementIntent {
    /// Accepts only finite, unit-bounded ordinary movement controls.
    pub fn is_valid(self) -> bool {
        self.yaw.is_finite()
            && self.axes.iter().all(|axis| axis.is_finite())
            && self.axes[0].hypot(self.axes[1]) <= 1.0001
    }
}

/// A callback's effects are applied only within the current host-owned camera lease.
/// Rotation is consumed once; retained look and movement require INPUT_INTENTS.
/// The host expires retained controls and revokes them at input/world ownership boundaries.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<LookIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<MovementIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<super::native_frame::Delta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewPose>,
    /// Only meaningful with `view`: native look input turns the presented view instead of the
    /// body, with pitch clamped to ±89.9°, and the next delivered frame's `look_delta` reports
    /// exactly the look applied since the previous delivered frame.
    #[serde(default)]
    pub capture_look: bool,
    /// Only valid with `view`: the ordinary movement, jump and sneak bindings stop moving the
    /// body and are still reported in the next frame's `move_axes`, `ascend` and `descend`.
    #[serde(default)]
    pub capture_movement: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_tilt: Option<ViewTilt>,
    #[serde(default)]
    pub preserve_teleport_rotation: bool,
}

impl Effects {
    /// Uses the owning camera bounds for every supported effect.
    pub fn is_valid(&self) -> bool {
        self.look.is_none_or(LookIntent::is_valid)
            && self.movement.is_none_or(MovementIntent::is_valid)
            && !(self.look.is_some() && self.rotation.is_some())
            && self
                .rotation
                .is_none_or(super::native_frame::Delta::is_valid)
            && self.view.is_none_or(ViewPose::is_valid)
            && (self.view.is_some() || !self.capture_movement)
            && self.view_tilt.is_none_or(ViewTilt::is_valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_output_fixture_matches_the_sdk() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../native-camera-v1.json")).unwrap();
        assert_eq!(fixture["capabilities"], CAPABILITY);
        let effects: Effects = serde_json::from_value(fixture["effects"].clone()).unwrap();
        assert!(effects.is_valid());
        assert_eq!(serde_json::to_value(effects).unwrap(), fixture["effects"]);
    }

    #[test]
    fn effects_reject_unbounded_camera_requests() {
        assert!(Effects::default().is_valid());
        let view = ViewPose {
            position: [1.0, 65.0, -2.0],
            yaw: 0.5,
            pitch: -0.25,
            velocity: [0.0, 0.0, 10.0],
        };
        assert!(
            Effects {
                view: Some(view),
                capture_look: true,
                capture_movement: true,
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !Effects {
                capture_movement: true,
                ..Default::default()
            }
            .is_valid()
        );
        for invalid in [
            ViewPose {
                position: [f32::NAN, 0.0, 0.0],
                ..view
            },
            ViewPose { pitch: 2.0, ..view },
            ViewPose {
                velocity: [0.0, MAX_VIEW_SPEED + 1.0, 0.0],
                ..view
            },
        ] {
            assert!(
                !Effects {
                    view: Some(invalid),
                    ..Default::default()
                }
                .is_valid()
            );
        }
        let tilt = ViewTilt {
            roll: 14.0,
            pitch: 0.0,
            suppress_hurt_shake: true,
        };
        assert!(
            Effects {
                view_tilt: Some(tilt),
                ..Default::default()
            }
            .is_valid()
        );
        for roll in [f32::INFINITY, crate::MAX_VIEW_TILT_DEGREES + 1.0] {
            assert!(
                !Effects {
                    view_tilt: Some(ViewTilt { roll, ..tilt }),
                    ..Default::default()
                }
                .is_valid()
            );
        }
        assert!(serde_json::from_str::<Effects>(r#"{"unknown":true}"#).is_err());
        assert!(
            serde_json::from_str::<Effects>(r#"{"detached":{"kind":"orbit","distance":4.0}}"#)
                .is_err()
        );
    }
    #[test]
    fn input_intents_reject_invalid_axes_and_conflicting_look_owners() {
        let look = LookIntent {
            yaw: 1.0,
            pitch: 0.0,
            speed: 3.0,
        };
        let movement = MovementIntent {
            yaw: 1.0,
            axes: [0.0, 1.0],
            jump: true,
            sneak: false,
            sprint: false,
        };
        assert!(
            Effects {
                look: Some(look),
                movement: Some(movement),
                ..Default::default()
            }
            .is_valid()
        );
        assert!(
            !MovementIntent {
                axes: [1.0, 1.0],
                ..movement
            }
            .is_valid()
        );
        assert!(
            !MovementIntent {
                yaw: f32::NAN,
                ..movement
            }
            .is_valid()
        );
        assert!(
            !LookIntent {
                speed: f32::INFINITY,
                ..look
            }
            .is_valid()
        );
        assert!(
            !Effects {
                look: Some(look),
                rotation: Some(super::super::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0
                }),
                ..Default::default()
            }
            .is_valid()
        );
    }
}
