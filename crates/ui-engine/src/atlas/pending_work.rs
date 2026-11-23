//! Background image work a later frame collects, which a retained frame must not outlive.

use super::ServerAtlas;

impl ServerAtlas {
    /// Starts a frame's texture requests: misses left for a later frame accumulate until the
    /// next frame starts, whichever request left them.
    pub fn begin_frame(&mut self) {
        self.decodes.deferred = false;
    }

    /// Whether decode work a later require would collect, or a decode left for a later frame,
    /// is waiting: a frame drawn without it may draw differently once it lands.
    pub fn completions_waiting(&self) -> bool {
        self.decodes.deferred || self.decodes.jobs.completion_waiting()
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
