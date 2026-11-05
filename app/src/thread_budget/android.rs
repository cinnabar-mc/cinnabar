//! Android frame-thread scheduling. Bedrock runs its frame and rendering threads above normal
//! worker priority; Android expresses that through per-thread niceness.

/// Android's `THREAD_PRIORITY_DISPLAY`, the niceness the platform gives its own display threads.
const FRAME_THREAD_NICENESS: i32 = -4;

/// The niceness that raises a frame thread without lowering one the platform already raised.
fn raised_niceness(current: i32) -> i32 {
    current.min(FRAME_THREAD_NICENESS)
}

/// Raises the calling thread to frame priority; failures keep the inherited priority.
#[cfg(target_os = "android")]
pub(super) fn prepare_frame_thread() {
    // SAFETY: gettid has no preconditions, and both calls address only the calling task.
    let task = unsafe { libc::gettid() } as libc::id_t;
    let current = unsafe { libc::getpriority(libc::PRIO_PROCESS, task) };
    let raised = raised_niceness(current);
    if raised == current {
        return;
    }
    // SAFETY: This changes only the calling task's niceness.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, task, raised) } != 0 {
        bevy::log::warn!(
            error = %std::io::Error::last_os_error(),
            "could not raise a frame thread's scheduling priority"
        );
    }
}

/// Exclusive execution keeps this one-time policy on the render schedule's own thread.
#[cfg(target_os = "android")]
pub(super) fn prepare_render_thread(_world: &mut bevy::prelude::World) {
    prepare_frame_thread();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Normal and background threads rise to frame priority; already-raised threads keep theirs.
    #[test]
    fn frame_threads_rise_without_lowering_a_platform_boost() {
        assert_eq!(raised_niceness(0), FRAME_THREAD_NICENESS);
        assert_eq!(raised_niceness(10), FRAME_THREAD_NICENESS);
        assert_eq!(raised_niceness(-10), -10);
    }
}
