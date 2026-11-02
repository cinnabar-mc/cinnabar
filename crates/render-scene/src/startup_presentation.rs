//! Startup feedback shared by the renderer and the main-world lifecycle gate.

use bevy::prelude::Resource;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use web_time::Instant;

/// Counts successful primary-window presentations whose submitted GPU work has completed.
#[derive(Clone, Default, Resource)]
pub struct StartupPresentation(Arc<Feedback>);

#[derive(Default)]
struct Feedback {
    frames: AtomicU8,
    content: Mutex<Content>,
}

#[derive(Default)]
struct Content {
    epoch: u64,
    active: bool,
    presented: Option<Instant>,
    mode_epoch: u64,
    mode_pending: bool,
}

impl StartupPresentation {
    /// Starts a display-mode attempt; older extracted frames cannot complete this attempt.
    pub fn begin_mode_change(&self) -> u64 {
        let mut content = self.0.content.lock().expect("startup content lock");
        content.mode_epoch += 1;
        content.mode_pending = true;
        content.mode_epoch
    }

    /// The mode attempt for which the renderer still needs to submit a presentation callback.
    pub fn pending_mode_change(&self) -> Option<u64> {
        let content = self.0.content.lock().expect("startup content lock");
        content.mode_pending.then_some(content.mode_epoch)
    }

    /// Accepts only a GPU-completed presentation extracted after this display-mode request.
    pub fn record_mode_frame(&self, epoch: u64) {
        let mut content = self.0.content.lock().expect("startup content lock");
        if content.mode_epoch == epoch {
            content.mode_pending = false;
        }
    }

    /// Confirms that this attempt, rather than an earlier in-flight frame, completed.
    pub fn mode_change_completed(&self, epoch: u64) -> bool {
        let content = self.0.content.lock().expect("startup content lock");
        content.mode_epoch == epoch && !content.mode_pending
    }

    /// The first presented frame and two more establish that rendering kept making progress.
    pub const REQUIRED_FRAMES: u8 = 3;

    /// Records one render-completion callback, retaining only the small startup frame count.
    pub fn record_completed_frame(&self) {
        let _ = self
            .0
            .frames
            .try_update(Ordering::Release, Ordering::Relaxed, |frames| {
                (frames < Self::REQUIRED_FRAMES).then_some(frames + 1)
            });
    }

    /// Starts a new native-surface lifetime, invalidating both pending and published feedback.
    pub fn replace_content_surface(&self, active: bool) {
        let mut content = self.0.content.lock().expect("startup content lock");
        content.epoch += 1;
        content.active = active;
        content.presented = None;
    }

    /// Identifies the currently usable native surface; hidden/offscreen views have none.
    pub fn content_epoch(&self) -> Option<u64> {
        let content = self.0.content.lock().expect("startup content lock");
        content.active.then_some(content.epoch)
    }

    /// Accepts GPU completion only for the surface that submitted the destination frame.
    pub fn record_content_frame(&self, epoch: u64, completed: Instant) {
        let mut content = self.0.content.lock().expect("startup content lock");
        if content.active && content.epoch == epoch {
            content.presented.get_or_insert(completed);
        }
    }

    /// Clear-only frames and completions from retired surfaces cannot dismiss feedback.
    pub fn has_presented_content(&self) -> bool {
        let content = self.0.content.lock().expect("startup content lock");
        content.active && content.presented.is_some()
    }

    /// The first GPU-completed content frame for the current native surface.
    pub fn content_presented_at(&self) -> Option<Instant> {
        let content = self.0.content.lock().expect("startup content lock");
        content.active.then_some(content.presented).flatten()
    }

    /// Whether enough presented frames have completed to confirm client startup.
    pub fn is_complete(&self) -> bool {
        self.0.frames.load(Ordering::Acquire) >= Self::REQUIRED_FRAMES
    }

    /// Whether the first primary-window presentation has completed its submitted GPU work.
    pub fn has_presented(&self) -> bool {
        self.0.frames.load(Ordering::Acquire) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_the_first_presentation_and_following_frames() {
        let feedback = StartupPresentation::default();
        let render_feedback = feedback.clone();
        assert!(!feedback.is_complete());
        assert!(!feedback.has_presented());
        assert!(!feedback.has_presented_content());
        for _ in 1..StartupPresentation::REQUIRED_FRAMES {
            render_feedback.record_completed_frame();
            assert!(feedback.has_presented());
            assert!(!feedback.is_complete());
        }
        render_feedback.record_completed_frame();
        assert!(feedback.is_complete());
        render_feedback.record_completed_frame();
        assert!(feedback.is_complete());
        assert!(!feedback.has_presented_content());
        feedback.replace_content_surface(true);
        render_feedback
            .record_content_frame(feedback.content_epoch().unwrap(), web_time::Instant::now());
        assert!(feedback.has_presented_content());
    }
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn mode_changes_require_a_presentation_from_their_own_extracted_frame() {
        let feedback = StartupPresentation::default();
        feedback.record_completed_frame();
        let old = feedback.begin_mode_change();
        let current = feedback.begin_mode_change();
        feedback.record_mode_frame(old);
        assert!(!feedback.mode_change_completed(current));
        assert_eq!(feedback.pending_mode_change(), Some(current));
        feedback.record_mode_frame(current);
        assert!(feedback.mode_change_completed(current));
        assert_eq!(feedback.pending_mode_change(), None);
    }

    #[test]
    fn old_surface_callbacks_and_unconsumed_readiness_do_not_survive_replacement() {
        let feedback = StartupPresentation::default();
        feedback.replace_content_surface(true);
        let old = feedback.content_epoch().unwrap();
        feedback.record_content_frame(old, web_time::Instant::now());
        assert!(feedback.has_presented_content());
        feedback.replace_content_surface(false);
        assert!(!feedback.has_presented_content());
        assert_eq!(feedback.content_epoch(), None);
        feedback.record_content_frame(old, web_time::Instant::now());
        assert!(!feedback.has_presented_content());
        feedback.replace_content_surface(true);
        let new = feedback.content_epoch().unwrap();
        feedback.record_content_frame(old, web_time::Instant::now());
        assert!(!feedback.has_presented_content());
        feedback.record_content_frame(new, web_time::Instant::now());
        assert!(feedback.has_presented_content());
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn first_content_completion_keeps_its_timestamp_until_the_surface_changes() {
        let feedback = StartupPresentation::default();
        let first = Instant::now();
        feedback.replace_content_surface(true);
        let epoch = feedback.content_epoch().unwrap();
        feedback.record_content_frame(epoch, first);
        feedback.record_content_frame(epoch, first + Duration::from_millis(5));
        assert_eq!(feedback.content_presented_at(), Some(first));
        feedback.replace_content_surface(true);
        feedback.record_content_frame(epoch, first);
        assert_eq!(feedback.content_presented_at(), None);
    }
}
