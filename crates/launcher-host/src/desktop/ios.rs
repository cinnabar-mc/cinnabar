//! UIKit URL handoff. iOS apps cannot start another process, so the shared `open` tool fallback
//! never reaches a browser there.

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSDictionary, NSString, NSURL};
use std::sync::mpsc;

/// Asks UIKit to open `url` and blocks the calling worker until UIKit reports whether an app
/// accepted it. Must not run on the main thread, which completes the request.
pub(super) fn open_url(url: &str) -> bool {
    let (sender, received) = mpsc::channel();
    let url = url.to_owned();
    DispatchQueue::main().exec_async(move || {
        let Some(target) = NSURL::URLWithString(&NSString::from_str(&url)) else {
            tracing::warn!("UIKit rejected the URL {url}");
            let _ = sender.send(false);
            return;
        };
        let app: Option<Retained<AnyObject>> =
            unsafe { msg_send![class!(UIApplication), sharedApplication] };
        let Some(app) = app else {
            let _ = sender.send(false);
            return;
        };
        let options = NSDictionary::<NSString, AnyObject>::new();
        // UIKit runs the handler on the main queue once the request is resolved.
        let handler = RcBlock::new(move |opened: Bool| {
            let _ = sender.send(opened.as_bool());
        });
        unsafe {
            let _: () = msg_send![
                &*app,
                openURL: &*target,
                options: &*options,
                completionHandler: &*handler
            ];
        }
    });
    received.recv().unwrap_or(false)
}
