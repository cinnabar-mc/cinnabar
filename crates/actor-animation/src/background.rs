//! Background preparation that leaves the frame thread.

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// Preparation waiting for the browser's idle job driver.
    static JOBS: std::cell::RefCell<std::collections::VecDeque<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// Runs `work` on Rayon's pool. Single-threaded browsers queue it for [`run_browser_job`]
/// instead: their global pool has no worker thread, so a spawned job would never run and its
/// result (a remote player's skin, for example) would never arrive.
pub(crate) fn spawn(work: impl FnOnce() + Send + 'static) {
    #[cfg(target_arch = "wasm32")]
    JOBS.with_borrow_mut(|jobs| jobs.push_back(Box::new(work)));
    #[cfg(not(target_arch = "wasm32"))]
    rayon::spawn(work);
}

/// Runs one queued skin or cape preparation after the browser's event loop yields; returns
/// whether one ran. Native builds have no queue.
pub fn run_browser_job() -> bool {
    #[cfg(target_arch = "wasm32")]
    if let Some(work) = JOBS.with_borrow_mut(std::collections::VecDeque::pop_front) {
        work();
        return true;
    }
    false
}
