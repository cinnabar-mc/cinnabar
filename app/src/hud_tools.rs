//! Function-key client tools: screenshot capture (F2) and the F3 debug overlay, plus the local
//! worlds' menu thumbnails, which share the screenshot's frame capture.

mod debug_overlay;
mod screenshot;
mod world_thumbnail;

#[cfg(all(test, feature = "developer-control"))]
pub(crate) use screenshot::configure_frame_capture;
#[cfg(feature = "developer-control")]
pub(crate) use screenshot::{FrameCapture, FrameCaptureSet, sdr, write_png};

use std::path::PathBuf;

use bevy::prelude::{App, Plugin};

pub(crate) struct HudToolsPlugin {
    pub screenshots_dir: PathBuf,
    pub debug_overlay: bool,
}

impl Plugin for HudToolsPlugin {
    fn build(&self, app: &mut App) {
        screenshot::configure(app, self.screenshots_dir.clone());
        world_thumbnail::configure(app);
        if self.debug_overlay {
            debug_overlay::configure(app);
        }
    }
}
