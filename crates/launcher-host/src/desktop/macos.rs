//! The in-process Open panel. Its selection lets this process read the file even in a
//! privacy-protected folder such as Downloads; a picker in a helper process grants only itself.

use objc2_app_kit::{NSApplication, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::NSString;
use std::path::PathBuf;

/// Returns the chosen files, empty on cancellation; blocks until the panel closes.
pub(super) fn pick_files(prompt: &str, multiple: bool) -> Vec<PathBuf> {
    dispatch2::run_on_main(|mtm| {
        let previous = NSApplication::sharedApplication(mtm).keyWindow();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(multiple);
        panel.setMessage(Some(&NSString::from_str(prompt)));
        let paths = if panel.runModal() == NSModalResponseOK {
            panel
                .URLs()
                .iter()
                .filter_map(|url| url.to_file_path())
                .collect()
        } else {
            Vec::new()
        };
        if let Some(window) = previous {
            window.makeKeyAndOrderFront(None);
        }
        paths
    })
}
