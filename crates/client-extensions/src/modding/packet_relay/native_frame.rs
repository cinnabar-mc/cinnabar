//! Latest-value native callbacks: neither frame input nor results can form a backlog.

use super::Request;
use arc_swap::ArcSwapOption;
use mod_api::native_camera::Effects;
use mod_host::NativeFrame as Frame;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::Sender,
};
use std::time::{Duration, Instant};

/// Relative rotations older than two client frames cannot move a new camera pose.
const MAX_RESULT_AGE: u64 = 2;

/// Independent effects published from one serialized guest observation.
#[derive(Default)]
pub struct FrameOutput {
    pub camera: Option<Effects>,
    /// The sequence of the frame whose callback produced `camera`.
    pub camera_sequence: u64,
    /// The sequence this exchange assigned to the frame it sent.
    pub sequence: u64,
    pub drawing: Option<Arc<mod_api::native_render::Scene>>,
}

/// A session-owned sender, weakly reachable by the app without retaining the session.
pub(super) struct FrameLink {
    requests: Sender<Request>,
    frames: Arc<NativeFrames>,
}

impl FrameLink {
    /// Whether the session negotiated native frames and still holds its source grants.
    pub(super) fn available(&self) -> bool {
        self.frames.available()
    }

    /// Binds one mailbox to the already serialized packet worker.
    pub(super) fn new(requests: Sender<Request>, frames: Arc<NativeFrames>) -> Self {
        Self { requests, frames }
    }

    /// Publishes without blocking; a busy worker sees only the newest pending input.
    pub(super) fn exchange(&self, frame: Option<Frame>) -> Option<FrameOutput> {
        let scope = frame
            .as_ref()
            .filter(|frame| frame.is_valid())
            .map(|frame| (frame.session, frame.dimension));
        let (output, wake) = self.frames.exchange(frame)?;
        if wake && self.requests.send(Request::NativeFrame).is_err() {
            self.frames.deactivate();
            return None;
        }
        let drawing = self.frames.drawing.load_full().filter(|scene| {
            self.frames.available()
                && !self.frames.invalidate.load(Ordering::Acquire)
                && scope == Some((scene.session, scene.dimension))
        });
        Some(FrameOutput {
            camera: output,
            camera_sequence: self.frames.source.load(Ordering::Acquire),
            sequence: self.frames.sequence.load(Ordering::Acquire),
            drawing,
        })
    }

    /// Revokes native work synchronously when the session requests stop.
    pub(super) fn deactivate(&self) {
        self.frames.deactivate();
    }
}

#[derive(Default)]
struct State {
    epoch: u64,
    scope: Option<(u64, i32, bool)>,
    attack_held: Option<bool>,
    pending: Option<Input>,
    wake_pending: bool,
}

/// One immutable coalesced observation and its host-only input ownership fence.
pub(super) struct Input {
    pub(super) epoch: u64,
    sequence: u64,
    observed_at: Instant,
    pub(super) frame: Option<Frame>,
}

impl Input {
    /// Preserves an input-loss reset even when newer valid input replaced the unavailable frame.
    pub(super) fn dispatch<R, E>(
        &self,
        previous_epoch: &mut Option<u64>,
        mut callback: impl FnMut(Option<&Frame>) -> Result<R, E>,
    ) -> Result<R, E> {
        if *previous_epoch != Some(self.epoch) && self.frame.is_some() {
            callback(None)?;
        }
        *previous_epoch = Some(self.epoch);
        callback(self.frame.as_ref())
    }
}

struct Output {
    observed_at: Instant,
    sequence: u64,
    scope: (u64, i32, bool),
    attack_held: bool,
    effects: Effects,
    rotation_taken: AtomicBool,
}

/// Shared data holds no sender and cannot keep the packet worker's channel alive.
pub(super) struct NativeFrames {
    state: Mutex<State>,
    output: ArcSwapOption<Output>,
    drawing: ArcSwapOption<mod_api::native_render::Scene>,
    sequence: AtomicU64,
    /// The input sequence behind the effects the last exchange returned.
    source: AtomicU64,
    /// Look and real seconds of frames a busy worker turned away, folded into the next frame.
    carry: Mutex<Increments>,
    ready: AtomicBool,
    stopped: AtomicBool,
    invalidate: AtomicBool,
    suspend: AtomicBool,
    observed_suspended: AtomicBool,
    authority: Arc<AtomicBool>,
}

impl NativeFrames {
    /// A stopped or revoked session cannot regain authority through an in-flight activation.
    fn available(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.stopped.load(Ordering::Acquire)
            && self.authority.load(Ordering::Acquire)
    }

    /// Starts unavailable until the guest successfully starts with negotiated native support.
    pub(super) fn new(authority: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(State::default()),
            output: ArcSwapOption::empty(),
            drawing: ArcSwapOption::empty(),
            sequence: AtomicU64::new(0),
            source: AtomicU64::new(0),
            carry: Mutex::new(Increments::default()),
            ready: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            invalidate: AtomicBool::new(false),
            suspend: AtomicBool::new(false),
            observed_suspended: AtomicBool::new(false),
            authority,
        }
    }

    /// Makes only a successfully started, capable guest available to client frames.
    pub(super) fn activate(&self, ready: bool) {
        self.ready.store(
            ready && !self.stopped.load(Ordering::Acquire),
            Ordering::Release,
        );
    }

    /// Clears all pending effects before another session can acquire camera ownership.
    pub(super) fn deactivate(&self) {
        self.ready.store(false, Ordering::Release);
        self.stopped.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.epoch = state.epoch.wrapping_add(1);
        state.pending = None;
        self.output.store(None);
        self.drawing.store(None);
        state.wake_pending = false;
    }

    /// Packet and timed callbacks may change guest policy; older observations cannot outlive them.
    pub(super) fn fence_guest_state(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(frame) = state.pending.take().and_then(|input| input.frame) {
            self.carry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .add(Increments::of(&frame));
        }
        if let Some(output) = self.output.load_full() {
            output.rotation_taken.store(true, Ordering::Release);
        }
    }

    /// Reads retained configuration without contending with worker completion.
    fn read_output(&self, frame: Option<&Frame>, sequence: u64) -> Option<Effects> {
        let frame = frame?;
        if !self.available() || self.invalidate.load(Ordering::Acquire) {
            return None;
        }
        let output = self.output.load_full()?;
        if (output.scope.0, output.scope.1) != (frame.session, frame.dimension) {
            return None;
        }
        let mut effects = output.effects.clone();
        if self.suspend.load(Ordering::Acquire)
            || frame.suspended
            || output.scope.2
            || output.attack_held != frame.attack_held
            || output.observed_at.elapsed() > Duration::from_millis(250)
        {
            effects.look = None;
            effects.movement = None;
        }
        if self.suspend.load(Ordering::Acquire) || frame.suspended || output.scope.2 {
            effects.view = None;
            effects.capture_look = false;
            effects.capture_movement = false;
        }
        if self.suspend.load(Ordering::Acquire)
            || frame.suspended
            || output.scope.2
            || output.attack_held != frame.attack_held
            || sequence.wrapping_sub(output.sequence) > MAX_RESULT_AGE
            || output.rotation_taken.swap(true, Ordering::AcqRel)
        {
            effects.rotation = None;
        }
        self.source.store(output.sequence, Ordering::Release);
        (effects != Effects::default()).then_some(effects)
    }

    /// Coalesces work while retaining view configuration until its ownership is released.
    /// A delivered frame's look and elapsed time include those of every frame coalesced into
    /// it since the previous delivered frame, and start from zero after a reset.
    fn exchange(&self, frame: Option<Frame>) -> Option<(Option<Effects>, bool)> {
        if !self.available() {
            return None;
        }
        let mut frame = frame.filter(Frame::is_valid);
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if frame.is_none() {
            self.invalidate.store(true, Ordering::Release);
        }
        let suspended = frame.as_ref().is_some_and(|frame| frame.suspended);
        if !self.observed_suspended.swap(suspended, Ordering::AcqRel) && suspended {
            self.suspend.store(true, Ordering::Release);
        }
        let Ok(mut state) = self.state.try_lock() else {
            if let Some(frame) = frame.as_ref() {
                self.carry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .add(Increments::of(frame));
            }
            return Some((self.read_output(frame.as_ref(), sequence), false));
        };
        let scope = frame
            .as_ref()
            .map(|frame| (frame.session, frame.dimension, frame.suspended));
        let attack_held = frame.as_ref().map(|frame| frame.attack_held);
        if state.attack_held != attack_held {
            state.attack_held = attack_held;
            if let Some(output) = self.output.load_full() {
                output.rotation_taken.store(true, Ordering::Release);
            }
        }
        let invalidated = self.invalidate.load(Ordering::Acquire);
        if scope.is_none() && state.scope.is_none() && sequence != 1 {
            self.output.store(None);
            return Some((None, false));
        }
        let reset = state.scope != scope || invalidated || self.suspend.load(Ordering::Acquire);
        let carry = std::mem::take(&mut *self.carry.lock().unwrap_or_else(PoisonError::into_inner));
        if let Some(frame) = frame.as_mut() {
            let coalesced = state
                .pending
                .as_ref()
                .and_then(|input| input.frame.as_ref())
                .filter(|_| !reset)
                .map(Increments::of)
                .unwrap_or_default();
            let mut increments = Increments::default();
            if !reset {
                increments.add(Increments::of(frame));
                increments.add(carry);
                increments.add(coalesced);
            }
            increments.write(frame);
        }
        if reset {
            let same_world = state
                .scope
                .zip(scope)
                .is_some_and(|(old, new)| (old.0, old.1) == (new.0, new.1));
            if !same_world || invalidated {
                self.drawing.store(None);
            }
            state.epoch = state.epoch.wrapping_add(1);
            state.scope = scope;
            let passive = if same_world {
                self.output.load_full().map(|output| {
                    let mut effects = output.effects.clone();
                    effects.rotation = None;
                    effects.view = None;
                    effects.capture_look = false;
                    effects.capture_movement = false;
                    effects.look = None;
                    effects.movement = None;
                    Arc::new(Output {
                        observed_at: output.observed_at,
                        sequence,
                        scope: scope.unwrap(),
                        attack_held: attack_held.unwrap(),
                        effects,
                        rotation_taken: AtomicBool::new(true),
                    })
                })
            } else {
                None
            };
            self.output.store(passive);
        }
        self.invalidate.store(false, Ordering::Release);
        self.suspend.store(false, Ordering::Release);
        let output = self.read_output(frame.as_ref(), sequence);
        state.pending = Some(Input {
            observed_at: Instant::now(),
            epoch: state.epoch,
            sequence,
            frame,
        });
        let wake = !state.wake_pending;
        state.wake_pending = true;
        Some((output, wake))
    }

    /// Removes the newest pending input while allowing the app to queue one later wake.
    pub(super) fn take_input(&self) -> Option<Input> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.wake_pending = false;
        let input = state.pending.take();
        self.available().then_some(input).flatten()
    }

    /// Publishes immutable drawings without turning camera-input suspension into world loss.
    pub(super) fn complete_drawing(
        &self,
        input: &Input,
        scene: Option<mod_api::native_render::Scene>,
    ) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.available()
            || self.invalidate.load(Ordering::Acquire)
            || state.epoch != input.epoch
        {
            return;
        }
        let previous = self.drawing.load_full();
        if previous.as_deref() != scene.as_ref() {
            self.drawing.store(scene.map(Arc::new));
        }
    }

    /// Publishes configuration for the current scope, aging only the relative rotation.
    pub(super) fn complete(&self, input: Input, effects: Option<Effects>) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if self.available()
            && !self.invalidate.load(Ordering::Acquire)
            && !self.suspend.load(Ordering::Acquire)
            && input.epoch == state.epoch
            && input.frame.as_ref().map(|frame| frame.attack_held) == state.attack_held
        {
            self.output.store(effects.and_then(|effects| {
                let frame = input.frame.as_ref()?;
                Some(Arc::new(Output {
                    observed_at: input.observed_at,
                    sequence: input.sequence,
                    scope: (frame.session, frame.dimension, frame.suspended),
                    attack_held: frame.attack_held,
                    effects,
                    rotation_taken: AtomicBool::new(false),
                }))
            }));
        }
    }
}

/// Look and real seconds a frame reports since the previous delivered frame.
#[derive(Clone, Copy, Default)]
struct Increments {
    look: [f32; 2],
    elapsed: f32,
}

impl Increments {
    /// The increments `frame` carries.
    fn of(frame: &Frame) -> Self {
        Self {
            look: frame.look_delta,
            elapsed: frame.elapsed,
        }
    }

    /// Accumulates a later frame's increments.
    fn add(&mut self, other: Self) {
        self.look[0] += other.look[0];
        self.look[1] += other.look[1];
        self.elapsed += other.elapsed;
    }

    /// Reports these increments in `frame`.
    fn write(self, frame: &mut Frame) {
        frame.look_delta = self.look;
        frame.elapsed = self.elapsed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the wire fixture's exact 64-bit identities in a deterministic input.
    fn frame() -> Frame {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../../mod-api/native-frame-v2.json")).unwrap();
        {
            let mut frame: Frame = serde_json::from_value(fixture["frame"].clone()).unwrap();
            frame.attack_held = true;
            frame
        }
    }

    /// A finite presented view near the fixture frame's eye.
    fn view() -> mod_api::native_camera::ViewPose {
        mod_api::native_camera::ViewPose {
            position: [1.25, 70.5, -2.0],
            yaw: 0.5,
            pitch: 0.0,
            velocity: [0.0; 3],
        }
    }

    /// A passive current-frame view tilt.
    fn tilt() -> mod_api::native_camera::ViewTilt {
        mod_api::native_camera::ViewTilt {
            roll: 14.0,
            pitch: 0.0,
            suppress_hurt_shake: true,
        }
    }

    #[test]
    fn drawings_survive_input_suspension_but_not_coalesced_world_loss() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        let observation = frame();
        mailbox.exchange(Some(observation.clone()));
        let input = mailbox.take_input().unwrap();
        let scene = mod_api::native_render::Scene {
            version: 1,
            session: observation.session,
            dimension: observation.dimension,
            ..Default::default()
        };
        mailbox.complete_drawing(&input, Some(scene.clone()));
        let first = mailbox.drawing.load_full().unwrap();
        mailbox.complete_drawing(&input, Some(scene));
        assert!(Arc::ptr_eq(&first, &mailbox.drawing.load_full().unwrap()));
        let mut suspended = observation.clone();
        suspended.suspended = true;
        suspended.players.clear();
        suspended.attack_held = false;
        mailbox.exchange(Some(suspended));
        assert!(mailbox.drawing.load_full().is_some());
        {
            let _worker = mailbox.state.lock().unwrap();
            mailbox.exchange(None);
        }
        mailbox.exchange(Some(observation));
        assert!(mailbox.drawing.load_full().is_none());
        mailbox.deactivate();
        mailbox.complete_drawing(&input, Some((*first).clone()));
        assert!(mailbox.drawing.load_full().is_none());
    }

    #[test]
    fn presentation_survives_packet_fences_but_view_releases_on_input_loss() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame())).unwrap();
        let input = mailbox.take_input().unwrap();
        let effects = Effects {
            view: Some(view()),
            capture_look: true,
            capture_movement: true,
            view_tilt: Some(tilt()),
            ..Default::default()
        };
        mailbox.complete(input, Some(effects.clone()));
        mailbox.fence_guest_state();
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, Some(effects));
        let mut suspended = frame();
        suspended.suspended = true;
        suspended.players.clear();
        suspended.attack_held = false;
        let passive = mailbox.exchange(Some(suspended)).unwrap().0.unwrap();
        assert_eq!(passive.view, None);
        assert!(!passive.capture_look && !passive.capture_movement);
        assert_eq!(passive.rotation, None);
        assert_eq!(passive.view_tilt, Some(tilt()));
        assert_eq!(mailbox.exchange(None).unwrap().0, None);
    }

    #[test]
    fn retained_views_survive_worker_contention_and_rotation_expiry() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        let effects = Effects {
            view: Some(view()),
            ..Default::default()
        };
        mailbox.complete(input, Some(effects.clone()));
        {
            let _worker_completing = mailbox.state.lock().unwrap();
            for _ in 0..8 {
                assert_eq!(
                    mailbox.exchange(Some(frame())),
                    Some((Some(effects.clone()), false))
                );
            }
        }
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, Some(effects));
        let mut elsewhere = frame();
        elsewhere.dimension += 1;
        assert_eq!(mailbox.exchange(Some(elsewhere)).unwrap().0, None);
    }

    /// A delivered frame reports the look and real time of every frame since the previous
    /// delivery, including coalesced and turned-away frames; a reset starts from zero.
    #[test]
    fn delivered_frames_report_all_look_and_time_since_the_previous_delivery() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        let with = |look: [f32; 2], elapsed: f32| Frame {
            look_delta: look,
            elapsed,
            ..frame()
        };
        let delivered = |mailbox: &NativeFrames| {
            let frame = mailbox.take_input().unwrap().frame.unwrap();
            (frame.look_delta, frame.elapsed)
        };
        mailbox.exchange(Some(with([0.5, 0.5], 0.5)));
        assert_eq!(
            delivered(&mailbox),
            ([0.0; 2], 0.0),
            "the first frame starts a run"
        );
        mailbox.exchange(Some(with([0.1, -0.2], 0.01)));
        mailbox.exchange(Some(with([0.3, 0.1], 0.02)));
        {
            let _worker = mailbox.state.lock().unwrap();
            mailbox.exchange(Some(with([0.05, 0.0], 0.03)));
        }
        mailbox.exchange(Some(with([0.0, 0.25], 0.04)));
        let (look, elapsed) = delivered(&mailbox);
        assert!((look[0] - 0.45).abs() < 1e-6 && (look[1] - 0.15).abs() < 1e-6);
        assert!((elapsed - 0.1).abs() < 1e-6);
        mailbox.exchange(Some(with([0.2, 0.0], 0.05)));
        assert_eq!(delivered(&mailbox), ([0.2, 0.0], 0.05));
        mailbox.exchange(Some(with([1.0, 1.0], 1.0)));
        mailbox.exchange(None);
        mailbox.exchange(Some(with([0.3, 0.3], 0.3)));
        assert_eq!(delivered(&mailbox), ([0.0; 2], 0.0), "input loss resets");
    }

    #[test]
    fn guest_fences_preserve_queued_increments_once_and_resets_discard_them() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        mailbox.take_input().unwrap();
        let increment = Frame {
            look_delta: [0.125, -0.25],
            elapsed: 0.03125,
            ..frame()
        };
        for _ in 0..2 {
            mailbox.exchange(Some(increment.clone()));
            mailbox.fence_guest_state();
            mailbox.fence_guest_state();
            assert!(mailbox.take_input().is_none());
        }
        mailbox.exchange(Some(increment.clone()));
        let delivered = mailbox.take_input().unwrap().frame.unwrap();
        assert_eq!(delivered.look_delta, [0.375, -0.75]);
        assert_eq!(delivered.elapsed, 0.09375);
        mailbox.exchange(Some(increment.clone()));
        let delivered = mailbox.take_input().unwrap().frame.unwrap();
        assert_eq!(delivered.look_delta, increment.look_delta);
        assert_eq!(delivered.elapsed, increment.elapsed);

        for reset in [
            None,
            Some(Frame {
                dimension: 42,
                ..frame()
            }),
        ] {
            mailbox.exchange(Some(increment.clone()));
            mailbox.fence_guest_state();
            mailbox.exchange(reset);
            mailbox.exchange(Some(increment.clone()));
            let delivered = mailbox.take_input().unwrap().frame.unwrap();
            assert_eq!(delivered.look_delta, [0.0; 2]);
            assert_eq!(delivered.elapsed, 0.0);
        }
    }

    #[test]
    fn effects_name_the_frame_whose_callback_produced_them() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        let source = input.sequence;
        let effects = Effects {
            view: Some(view()),
            ..Default::default()
        };
        mailbox.complete(input, Some(effects.clone()));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, Some(effects));
        assert_eq!(mailbox.source.load(Ordering::Acquire), source);
        assert_eq!(mailbox.sequence.load(Ordering::Acquire), source + 1);
    }

    #[test]
    fn frames_coalesce_and_results_are_consumed_once() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        assert_eq!(mailbox.exchange(Some(frame())), Some((None, true)));
        assert_eq!(mailbox.exchange(Some(frame())), Some((None, false)));
        let input = mailbox.take_input().unwrap();
        assert_eq!(input.sequence, 2);
        let delta = Effects {
            rotation: Some(mod_api::native_frame::Delta {
                yaw: 0.1,
                pitch: 0.0,
            }),
            ..Default::default()
        };
        mailbox.complete(input, Some(delta.clone()));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, Some(delta));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn loss_of_input_and_revocation_discard_in_flight_results() {
        let authority = Arc::new(AtomicBool::new(true));
        let mailbox = NativeFrames::new(Arc::clone(&authority));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        mailbox.exchange(None);
        mailbox.complete(
            input,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        authority.store(false, Ordering::Release);
        assert_eq!(mailbox.exchange(Some(frame())), None);
    }

    #[test]
    fn stale_results_do_not_move_a_later_frame() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        for _ in 0..3 {
            mailbox.exchange(Some(frame()));
        }
        mailbox.complete(
            input,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn input_loss_is_recorded_even_when_the_mailbox_is_busy() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        {
            let _locked = mailbox.state.lock().unwrap();
            assert_eq!(mailbox.exchange(None), Some((None, false)));
        }
        mailbox.complete(
            input,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }

    #[test]
    fn coalesced_input_loss_resets_the_guest_before_the_next_valid_frame() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let initial = mailbox.take_input().unwrap();
        let mut previous_epoch = Some(initial.epoch);
        mailbox.exchange(None);
        mailbox.exchange(Some(frame()));
        let resumed = mailbox.take_input().unwrap();
        let mut calls = Vec::new();
        resumed
            .dispatch(&mut previous_epoch, |frame| {
                calls.push(frame.is_some());
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(calls, [false, true]);
    }

    #[test]
    fn packet_and_tick_state_work_discards_older_effects_without_resetting_target_scope() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        let epoch = input.epoch;
        mailbox.complete(
            input,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        mailbox.fence_guest_state();
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        mailbox.fence_guest_state();
        assert!(mailbox.take_input().is_none());
        mailbox.exchange(Some(frame()));
        let next = mailbox.take_input().unwrap();
        assert_eq!(next.epoch, epoch);
        let mut previous_epoch = Some(epoch);
        let mut calls = Vec::new();
        next.dispatch(&mut previous_epoch, |frame| {
            calls.push(frame.is_some());
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(calls, [true]);
    }

    #[test]
    fn attack_release_discards_cached_and_in_flight_held_input_effects() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        let mut released = frame();
        released.attack_held = false;
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        mailbox.complete(
            input,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        assert_eq!(mailbox.exchange(Some(released.clone())).unwrap().0, None);
        mailbox.exchange(Some(frame()));
        let in_flight = mailbox.take_input().unwrap();
        mailbox.exchange(Some(released.clone()));
        mailbox.complete(
            in_flight,
            Some(Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                ..Default::default()
            }),
        );
        assert_eq!(mailbox.exchange(Some(released)).unwrap().0, None);
    }

    #[test]
    fn stop_cannot_be_undone_by_an_in_flight_start() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.deactivate();
        mailbox.activate(true);
        assert_eq!(mailbox.exchange(Some(frame())), None);
    }
    #[test]
    fn contended_suspension_retires_cached_and_inflight_rotations() {
        for cached in [false, true] {
            let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
            mailbox.activate(true);
            mailbox.exchange(Some(frame()));
            let mut old = Some(mailbox.take_input().unwrap());
            let effects = Effects {
                rotation: Some(mod_api::native_frame::Delta {
                    yaw: 0.1,
                    pitch: 0.0,
                }),
                view_tilt: Some(tilt()),
                ..Default::default()
            };
            if cached {
                mailbox.complete(old.take().unwrap(), Some(effects.clone()));
            }
            {
                let _worker = mailbox.state.lock().unwrap();
                let mut suspended = frame();
                suspended.suspended = true;
                suspended.attack_held = false;
                suspended.players.clear();
                let result = mailbox.exchange(Some(suspended)).unwrap().0;
                assert!(result.is_none_or(|effect| effect.rotation.is_none()));
                let result = mailbox.exchange(Some(frame())).unwrap().0;
                assert!(
                    result.is_none_or(|effect| effect.rotation.is_none()),
                    "cached pre-suspension rotation escaped"
                );
            }
            if let Some(old) = old {
                mailbox.complete(old, Some(effects.clone()));
            }
            let result = mailbox.exchange(Some(frame())).unwrap().0;
            assert!(
                result.is_none_or(|effect| effect.rotation.is_none()),
                "in-flight pre-suspension rotation escaped"
            );
            let fresh = mailbox.take_input().unwrap();
            mailbox.complete(fresh, Some(effects));
            assert!(
                mailbox
                    .exchange(Some(frame()))
                    .unwrap()
                    .0
                    .unwrap()
                    .rotation
                    .is_some()
            );
        }
    }
    #[test]
    fn retained_input_survives_relative_step_consumption_and_expires() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let input = mailbox.take_input().unwrap();
        let effects = Effects {
            look: Some(mod_api::native_camera::LookIntent {
                yaw: 0.5,
                pitch: 0.0,
                speed: 3.0,
            }),
            movement: Some(mod_api::native_camera::MovementIntent {
                yaw: 0.5,
                axes: [0.0, 1.0],
                jump: false,
                sneak: false,
                sprint: true,
            }),
            ..Default::default()
        };
        mailbox.complete(input, Some(effects.clone()));
        for _ in 0..5 {
            mailbox.fence_guest_state();
            assert_eq!(
                mailbox.exchange(Some(frame())).unwrap().0,
                Some(effects.clone())
            );
        }
        let mut suspended = frame();
        suspended.suspended = true;
        suspended.attack_held = false;
        suspended.players.clear();
        assert_eq!(mailbox.exchange(Some(suspended)).unwrap().0, None);
        mailbox.exchange(Some(frame()));
        let mut input = mailbox.take_input().unwrap();
        input.observed_at -= Duration::from_secs(1);
        mailbox.complete(input, Some(effects));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
    }
    #[test]
    fn contended_suspension_retires_cached_and_inflight_controls() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        mailbox.exchange(Some(frame()));
        let old = mailbox.take_input().unwrap();
        let effects = Effects {
            movement: Some(mod_api::native_camera::MovementIntent {
                yaw: 0.0,
                axes: [0.0, 1.0],
                jump: true,
                sneak: false,
                sprint: false,
            }),
            ..Default::default()
        };
        mailbox.complete(old, Some(effects.clone()));
        mailbox.exchange(Some(frame()));
        let inflight = mailbox.take_input().unwrap();
        {
            let _worker = mailbox.state.lock().unwrap();
            let mut suspended = frame();
            suspended.suspended = true;
            suspended.attack_held = false;
            suspended.players.clear();
            assert_eq!(mailbox.exchange(Some(suspended)).unwrap().0, None);
            assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        }
        mailbox.complete(inflight, Some(effects));
        assert_eq!(mailbox.exchange(Some(frame())).unwrap().0, None);
        let fresh = mailbox.take_input().unwrap();
        mailbox.complete(
            fresh,
            Some(Effects {
                look: Some(mod_api::native_camera::LookIntent {
                    yaw: 0.1,
                    pitch: 0.0,
                    speed: 2.0,
                }),
                ..Default::default()
            }),
        );
        assert!(mailbox.exchange(Some(frame())).unwrap().0.is_some());
    }
    #[test]
    fn consecutive_suspended_frames_keep_the_drawing_publication_epoch() {
        let mailbox = NativeFrames::new(Arc::new(AtomicBool::new(true)));
        mailbox.activate(true);
        let mut suspended = frame();
        suspended.suspended = true;
        suspended.attack_held = false;
        suspended.players.clear();
        mailbox.exchange(Some(suspended.clone()));
        let input = mailbox.take_input().unwrap();
        mailbox.exchange(Some(suspended));
        let scene = mod_api::native_render::Scene {
            version: 1,
            session: frame().session,
            dimension: frame().dimension,
            ..Default::default()
        };
        mailbox.complete_drawing(&input, Some(scene));
        assert!(mailbox.drawing.load_full().is_some());
        assert_eq!(mailbox.take_input().unwrap().epoch, input.epoch);
    }
}
