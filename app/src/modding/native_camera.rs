//! Presents packet-camera output through the same presentation owners as local mods: a
//! detached view extrapolated between callbacks, with look capture accounted exactly.

use std::collections::VecDeque;

use bevy::prelude::{EulerRot, Quat, Vec3};
use client_presentation::{
    camera::{CameraSettingsAuthority, PITCH_LIMIT, PresentedView, ViewTilt},
    local_player::LocalViewPose,
};
use mod_api::native_camera::Effects;

/// Frames remembered for rebasing a callback's view onto later look and time.
const SENT_FRAMES: usize = 64;
/// Longest a view's position is extrapolated past the frame its callback observed.
const MAX_EXTRAPOLATION_SECONDS: f64 = 0.25;

/// When one frame was sent, and the look applied to presented views by then.
#[derive(Clone, Copy)]
struct Sent {
    sequence: u64,
    time: f64,
    applied: [f64; 2],
}

/// The packet mod's detached view between its callbacks, and the look the host applied to it.
#[derive(Default)]
pub(super) struct NativeCamera {
    /// Look applied since the last frame handed to the mod, and real seconds since then.
    unreported: Increments,
    /// The increments of the frame being exchanged, restored if it is not handed over.
    in_flight: Increments,
    /// Total look applied to presented views, as yaw and pitch radians.
    applied: [f64; 2],
    sent: VecDeque<Sent>,
    /// The active result keeps its baseline even after newer submissions evict its history.
    active: Option<Sent>,
    /// The presented heading last written to the look carrier, which look input then turns.
    carrier: Option<Carrier>,
}

#[derive(Clone, Copy, Default)]
struct Increments {
    look: [f32; 2],
    elapsed: f32,
}

#[derive(Clone, Copy)]
struct Carrier {
    rotation: Quat,
    yaw: f32,
    pitch: f32,
}

impl NativeCamera {
    /// Measures this frame's look input on the captured view and fills the frame's look and
    /// real time since the last frame handed to the mod.
    pub(super) fn prepare(
        &mut self,
        view: Option<&LocalViewPose>,
        seconds: f32,
        frame: Option<mod_host::NativeFrame>,
    ) -> Option<mod_host::NativeFrame> {
        if let (Some(carrier), Some(view)) = (self.carrier, view)
            && view.freelook_held()
            && view.look_rotation() != carrier.rotation
        {
            let (yaw, pitch) = view.look_angles();
            let delta = [yaw - carrier.yaw, pitch - carrier.pitch];
            self.applied[0] += f64::from(delta[0]);
            self.applied[1] += f64::from(delta[1]);
            self.unreported.look[0] += delta[0];
            self.unreported.look[1] += delta[1];
        }
        self.unreported.elapsed += seconds.max(0.0);
        let mut frame = frame?;
        self.in_flight = std::mem::take(&mut self.unreported);
        frame.look_delta = self.in_flight.look;
        frame.elapsed = self.in_flight.elapsed;
        Some(frame)
    }

    /// Records a sent frame for rebasing its callback's view; an unsent frame keeps its
    /// increments for the next one.
    pub(super) fn exchanged(&mut self, sequence: Option<u64>, now: f64) {
        let in_flight = std::mem::take(&mut self.in_flight);
        let Some(sequence) = sequence else {
            self.unreported.look[0] += in_flight.look[0];
            self.unreported.look[1] += in_flight.look[1];
            self.unreported.elapsed += in_flight.elapsed;
            return;
        };
        if self.sent.len() == SENT_FRAMES {
            self.sent.pop_front();
        }
        self.sent.push_back(Sent {
            sequence,
            time: now,
            applied: self.applied,
        });
    }

    /// Presents the latest callback's view, or restores the body's view without one.
    /// `source` is the sequence of the frame that callback observed.
    pub(super) fn apply(
        &mut self,
        camera: &mut CameraSettingsAuthority,
        view: Option<&mut LocalViewPose>,
        user_freelook: bool,
        effects: Option<&Effects>,
        source: u64,
        now: f64,
    ) {
        let presented = effects
            .and_then(|effects| Some((effects, effects.view?)))
            .map(|(effects, pose)| {
                let sent = self
                    .active
                    .filter(|sent| sent.sequence == source)
                    .unwrap_or_else(|| {
                        self.sent
                            .iter()
                            .rev()
                            .find(|sent| sent.sequence == source)
                            .copied()
                            .unwrap_or(Sent {
                                sequence: source,
                                time: now,
                                applied: self.applied,
                            })
                    });
                self.active = Some(sent);
                let residual = if effects.capture_look {
                    [
                        (self.applied[0] - sent.applied[0]) as f32,
                        (self.applied[1] - sent.applied[1]) as f32,
                    ]
                } else {
                    [0.0; 2]
                };
                let yaw = pose.yaw + residual[0];
                let pitch = (pose.pitch + residual[1]).clamp(-PITCH_LIMIT, PITCH_LIMIT);
                let ahead = (now - sent.time).clamp(0.0, MAX_EXTRAPOLATION_SECONDS) as f32;
                (
                    PresentedView {
                        position: Vec3::from_array(pose.position)
                            + Vec3::from_array(pose.velocity) * ahead,
                        rotation: Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0),
                        capture_look: effects.capture_look,
                        capture_movement: effects.capture_movement,
                    },
                    yaw,
                    pitch,
                )
            });
        if presented.is_none() {
            self.active = None;
        }
        self.carrier = None;
        let Some(view) = view else {
            camera.set_presented_view(None);
            return;
        };
        camera.apply_presented_view(view, presented.map(|(view, ..)| view), user_freelook);
        if let Some((presented, yaw, pitch)) = presented
            && presented.capture_look
        {
            view.set_look_angles(yaw, pitch);
            self.carrier = Some(Carrier {
                rotation: view.look_rotation(),
                yaw,
                pitch,
            });
        }
    }
}

/// Converts a mod's view tilt to the camera's presentation parameter.
pub(super) fn view_tilt(tilt: mod_api::native_camera::ViewTilt) -> ViewTilt {
    ViewTilt {
        roll_degrees: tilt.roll,
        pitch_degrees: tilt.pitch,
        suppress_hurt_shake: tilt.suppress_hurt_shake,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::native_camera::ViewPose;

    #[test]
    fn absent_native_effect_does_not_release_the_players_own_freelook() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        view.set_freelook(true);
        let rotation = Quat::from_rotation_y(0.5);
        view.set_look_rotation(rotation);
        NativeCamera::default().apply(&mut camera, Some(&mut view), false, None, 0, 0.0);
        assert_eq!(view.camera_rotation(), rotation);
    }

    /// A connected input-owned frame with no look or time of its own.
    fn frame() -> mod_host::NativeFrame {
        mod_host::NativeFrame {
            version: mod_api::native_frame::VERSION,
            session: 1,
            dimension: 0,
            eye: [0.0, 71.62, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            attack_held: false,
            seconds: 0.016,
            look_delta: [0.0; 2],
            move_axes: [0.0; 2],
            ascend: false,
            descend: false,
            elapsed: 0.0,
            players: Vec::new(),
            suspended: false,
        }
    }

    /// Retains a detached view with optional host look capture.
    fn effects(pose: ViewPose, capture_look: bool) -> Effects {
        Effects {
            view: Some(pose),
            capture_look,
            capture_movement: true,
            ..Default::default()
        }
    }

    /// Turns the look carrier as one frame of look input would.
    fn look(view: &mut LocalViewPose, yaw: f32, pitch: f32) {
        let (current_yaw, current_pitch) = view.look_angles();
        let pitch = (current_pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        view.turn_look_rotation(
            Quat::from_euler(EulerRot::YXZ, current_yaw + yaw, pitch, 0.0),
            yaw,
        );
    }

    /// Look input turns only the presented view; each frame reports exactly the look applied
    /// since the previous one, and a callback's view is rebased onto look applied after the
    /// frame it observed, so the camera never jumps.
    #[test]
    fn captured_look_turns_the_view_and_is_reported_exactly() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let body = view.rotation();
        let mut state = NativeCamera::default();
        let pose = ViewPose {
            position: [0.0, 70.0, 0.0],
            yaw: 0.25,
            pitch: 0.0,
            velocity: [0.0; 3],
        };
        let sent = state.prepare(Some(&view), 0.0, Some(frame())).unwrap();
        assert_eq!(sent.look_delta, [0.0; 2]);
        state.exchanged(Some(1), 0.0);
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(pose, true)),
            1,
            0.0,
        );
        assert!(camera.captures_look() && camera.captures_movement());
        // Two frames of look reach the view before the mod's callback for frame 2 returns.
        look(&mut view, 0.1, 0.05);
        let second = state.prepare(Some(&view), 0.016, Some(frame())).unwrap();
        assert!((second.look_delta[0] - 0.1).abs() < 1e-6);
        assert!((second.look_delta[1] - 0.05).abs() < 1e-6);
        assert!((second.elapsed - 0.016).abs() < 1e-6);
        state.exchanged(Some(2), 0.016);
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(pose, true)),
            1,
            0.016,
        );
        let presented = camera
            .presented_view()
            .unwrap()
            .rotation
            .to_euler(EulerRot::YXZ);
        assert!((presented.0 - 0.35).abs() < 1e-5 && (presented.1 - 0.05).abs() < 1e-5);
        look(&mut view, -0.02, 0.0);
        let third = state.prepare(Some(&view), 0.016, Some(frame())).unwrap();
        assert!((third.look_delta[0] + 0.02).abs() < 1e-6 && third.look_delta[1].abs() < 1e-6);
        state.exchanged(Some(3), 0.032);
        // The mod's callback for frame 2 adds that frame's look to its own yaw and pitch.
        let rebased = ViewPose {
            yaw: pose.yaw + second.look_delta[0],
            pitch: pose.pitch + second.look_delta[1],
            ..pose
        };
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(rebased, true)),
            2,
            0.032,
        );
        let presented = camera
            .presented_view()
            .unwrap()
            .rotation
            .to_euler(EulerRot::YXZ);
        assert!(
            (presented.0 - 0.33).abs() < 1e-5 && (presented.1 - 0.05).abs() < 1e-5,
            "look applied after the observed frame stays on the view"
        );
        assert_eq!(view.rotation(), body, "captured look never turns the body");
        state.apply(&mut camera, Some(&mut view), false, None, 0, 0.048);
        assert!(camera.presented_view().is_none() && !camera.captures_look());
        assert_eq!(view.camera_rotation(), body);
    }

    /// Pitch stops at the native limit and only the applied part is reported.
    #[test]
    fn captured_pitch_is_clamped_and_reports_only_what_was_applied() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let mut state = NativeCamera::default();
        let pose = ViewPose {
            position: [0.0; 3],
            yaw: 0.0,
            pitch: PITCH_LIMIT - 0.01,
            velocity: [0.0; 3],
        };
        state.prepare(Some(&view), 0.0, Some(frame()));
        state.exchanged(Some(1), 0.0);
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(pose, true)),
            1,
            0.0,
        );
        look(&mut view, 0.0, 0.5);
        let reported = state.prepare(Some(&view), 0.0, Some(frame())).unwrap();
        assert!((reported.look_delta[1] - 0.01).abs() < 1e-4);
        state.exchanged(Some(2), 0.0);
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(pose, true)),
            1,
            0.0,
        );
        let (_, pitch, _) = camera
            .presented_view()
            .unwrap()
            .rotation
            .to_euler(EulerRot::YXZ);
        assert!(pitch <= PITCH_LIMIT + 1e-5);
    }

    /// Between callbacks the position advances by the view's velocity from the frame its
    /// callback observed, so a late callback lands where the camera already is.
    #[test]
    fn position_is_extrapolated_from_the_observed_frame_by_velocity() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let mut state = NativeCamera::default();
        let pose = ViewPose {
            position: [10.0, 70.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            velocity: [4.0, 0.0, 0.0],
        };
        state.prepare(Some(&view), 0.0, Some(frame()));
        state.exchanged(Some(7), 1.0);
        for (now, x) in [(1.0, 10.0), (1.05, 10.2), (1.1, 10.4)] {
            state.apply(
                &mut camera,
                Some(&mut view),
                false,
                Some(&effects(pose, false)),
                7,
                now,
            );
            let position = camera.presented_view().unwrap().position;
            assert!((position.x - x).abs() < 1e-4, "{now}");
        }
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&effects(pose, false)),
            7,
            9.0,
        );
        let position = camera.presented_view().unwrap().position;
        assert!(
            (position.x - (10.0 + 4.0 * MAX_EXTRAPOLATION_SECONDS as f32)).abs() < 1e-4,
            "a stalled mod cannot carry the camera away"
        );
        assert!(!camera.captures_look());
        assert_eq!(view.rotation(), LocalViewPose::default().rotation());
    }

    /// A retained callback keeps its look and time origin when newer submissions fill history.
    #[test]
    fn retained_result_keeps_look_and_velocity_after_sent_history_eviction() {
        let mut camera = CameraSettingsAuthority::default();
        let mut view = LocalViewPose::default();
        let mut state = NativeCamera::default();
        let pose = ViewPose {
            position: [10.0, 70.0, 0.0],
            yaw: 0.25,
            pitch: 0.0,
            velocity: [4.0, 0.0, 0.0],
        };
        let retained = effects(pose, true);
        state.prepare(Some(&view), 0.0, Some(frame()));
        state.exchanged(Some(1), 1.0);
        state.apply(&mut camera, Some(&mut view), false, Some(&retained), 1, 1.0);
        for sequence in 2..=130 {
            let elapsed = (sequence - 1) as f64 * 0.002;
            let now = 1.0 + elapsed;
            look(&mut view, 0.001, 0.0005);
            state.prepare(Some(&view), 0.002, Some(frame()));
            state.exchanged(Some(sequence), now);
            state.apply(&mut camera, Some(&mut view), false, Some(&retained), 1, now);
            let presented = camera.presented_view().unwrap();
            let (yaw, pitch, _) = presented.rotation.to_euler(EulerRot::YXZ);
            assert!((yaw - (pose.yaw + (sequence - 1) as f32 * 0.001)).abs() < 1e-5);
            assert!((pitch - (sequence - 1) as f32 * 0.0005).abs() < 1e-5);
            let ahead = elapsed.min(MAX_EXTRAPOLATION_SECONDS) as f32;
            assert!((presented.position.x - (pose.position[0] + 4.0 * ahead)).abs() < 1e-5);
        }
        let replacement = effects(ViewPose { yaw: 0.75, ..pose }, true);
        state.apply(
            &mut camera,
            Some(&mut view),
            false,
            Some(&replacement),
            130,
            1.26,
        );
        let presented = camera.presented_view().unwrap();
        assert!((presented.rotation.to_euler(EulerRot::YXZ).0 - 0.75).abs() < 1e-5);
        assert!((presented.position.x - 10.008).abs() < 1e-5);
        assert_eq!(view.rotation(), LocalViewPose::default().rotation());
    }

    /// Real seconds and look of frames that never reached the mod join the next frame.
    #[test]
    fn unsent_frames_keep_their_time_and_look_for_the_next_one() {
        let view = LocalViewPose::default();
        let mut state = NativeCamera::default();
        state.prepare(Some(&view), 0.02, Some(frame()));
        state.exchanged(None, 0.0);
        state.prepare(Some(&view), 0.03, None);
        let next = state.prepare(Some(&view), 0.01, Some(frame())).unwrap();
        assert!((next.elapsed - 0.06).abs() < 1e-6);
    }
}
