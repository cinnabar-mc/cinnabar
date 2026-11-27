//! Background image work a later frame collects, which a retained frame must not outlive.

use super::ServerAtlas;

impl ServerAtlas {
    /// Starts a frame's texture requests: misses left for a later frame accumulate until the
    /// next frame starts, whichever request left them. Downloads a paint found loading stop
    /// waiting: this frame's paints request them again if they still draw them, so a download
    /// no screen draws any more is neither polled nor kept alive by repeated frames.
    pub fn begin_frame(&mut self) {
        self.decodes.deferred = false;
        self.untracked_remote.set(false);
        self.loading_remote.get_mut().clear();
    }

    /// Whether decode work a later require would collect, or a decode left for a later frame,
    /// is waiting: a frame drawn without it may draw differently once it lands.
    /// A download a paint found still loading counts once it settles, ready or failed; one the
    /// paint could not track counts at once, so each frame asks again until it can be tracked.
    pub fn completions_waiting(&self) -> bool {
        self.decodes.deferred
            || self.untracked_remote.get()
            || self.decodes.jobs.completion_waiting()
            || self.remote.as_ref().is_some_and(|remote| {
                self.loading_remote
                    .borrow()
                    .iter()
                    .any(|url| settled(remote.state(url)))
            })
    }

    /// Whether background image work this atlas submitted has not finished yet, for tests that
    /// wait for it before comparing frames.
    #[cfg(any(test, feature = "test-support"))]
    pub fn image_work_in_flight(&self) -> bool {
        self.decodes.jobs.in_flight()
            || self.native.in_flight()
            || self.outlines.in_flight()
            || self.local_reads.in_flight()
            || self
                .pack
                .as_ref()
                .is_some_and(|pack| pack.reads.in_flight())
    }
}

/// Whether a download finished, with or without an image.
fn settled(state: crate::sources::RemoteImage) -> bool {
    matches!(
        state,
        crate::sources::RemoteImage::Ready(_) | crate::sources::RemoteImage::Failed
    )
}
