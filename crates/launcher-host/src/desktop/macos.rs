//! The in-process Open panel. Its selection lets this process read the file even in a
//! privacy-protected folder such as Downloads; a picker in a helper process grants only itself.

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSModalResponse, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::NSString;
use std::{path::PathBuf, sync::mpsc};

/// Returns the chosen files, empty on cancellation; blocks only the calling worker.
///
/// The panel is modeless so the main thread keeps running frames while it is open.
pub(super) fn pick_files(prompt: &str, multiple: bool) -> Vec<PathBuf> {
    let (chosen, received) = mpsc::channel();
    let prompt = prompt.to_owned();
    DispatchQueue::main().exec_async(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let previous = NSApplication::sharedApplication(mtm).keyWindow();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(multiple);
        panel.setMessage(Some(&NSString::from_str(&prompt)));
        // AppKit releases the handler, and with it the panel, once it has run.
        let owner = panel.clone();
        let handler: RcBlock<dyn Fn(NSModalResponse)> = RcBlock::new(move |response| {
            let paths: Vec<PathBuf> = if response == NSModalResponseOK {
                owner
                    .URLs()
                    .iter()
                    .filter_map(|url| url.to_file_path())
                    .collect()
            } else {
                Vec::new()
            };
            if let Some(window) = &previous {
                window.makeKeyAndOrderFront(None);
            }
            let _ = chosen.send(paths);
        });
        panel.beginWithCompletionHandler(&handler);
    });
    received.recv().unwrap_or_default()
}
