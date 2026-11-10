//! Android frame-thread scheduling. Bedrock runs its main, frame and rendering threads above
//! normal worker priority; Android expresses that through per-thread niceness.

/// Android's `THREAD_PRIORITY_DISPLAY`, the niceness the platform gives its own display threads.
const FRAME_THREAD_NICENESS: i32 = -4;

/// The niceness that raises a frame thread without lowering one the platform already raised.
fn raised_niceness(current: i32) -> i32 {
    current.min(FRAME_THREAD_NICENESS)
}

/// Raises the calling thread to frame priority. Threads it spawns afterwards start at normal
/// priority instead of inheriting the boost. Failures keep the inherited priority.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
pub(super) fn prepare_frame_thread() {
    if let Err(error) = start_children_at_normal_priority() {
        bevy::log::warn!(%error, "frame thread children may inherit its scheduling priority");
        return;
    }
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

/// Sets the kernel's reset-on-fork flag, which starts threads spawned by the caller with zero
/// niceness when the caller's is negative. The caller keeps its policy and niceness.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
fn start_children_at_normal_priority() -> std::io::Result<()> {
    let param = libc::sched_param { sched_priority: 0 };
    // SAFETY: Thread ID zero addresses the calling thread, and `param` outlives the call.
    let status = unsafe {
        libc::sched_setscheduler(0, libc::SCHED_NORMAL | libc::SCHED_RESET_ON_FORK, &param)
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Exclusive execution keeps this one-time policy on the schedule's own thread.
#[cfg(target_os = "android")]
pub(super) fn prepare_schedule_thread(_world: &mut bevy::prelude::World) {
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

    /// Threads a frame thread spawns, such as shader-pack loaders, start at normal priority
    /// whether or not the host allowed the frame thread's own boost.
    #[cfg(target_os = "linux")]
    #[test]
    fn frame_thread_children_do_not_inherit_its_boost() {
        std::thread::spawn(|| {
            prepare_frame_thread();
            // SAFETY: Thread ID zero addresses this test thread.
            let policy = unsafe { libc::sched_getscheduler(0) };
            assert_ne!(policy & libc::SCHED_RESET_ON_FORK, 0);
            let child = std::thread::spawn(|| {
                // SAFETY: These calls address only the child's own task.
                unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) }
            })
            .join()
            .unwrap();
            assert!(child >= 0, "child inherited niceness {child}");
        })
        .join()
        .unwrap();
    }
}
