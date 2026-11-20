//! OS scheduling priority for world workers.
//!
//! Light and summary jobs run below the frame on background workers, in one of two ways.
//! Where a worker can return to normal priority it lowers itself only for those jobs, so
//! decode and mesh on the same worker keep normal priority: Windows, whose strict priorities
//! would otherwise stall them for seconds on a busy machine, and Android, whose processes may
//! raise their own niceness. Elsewhere unprivileged niceness cannot be raised back, so a
//! background worker is lowered slightly for life; proportional weighting keeps it progressing.

/// Prepares the calling background worker's priority as it starts. Returns whether the worker
/// lowers itself per job, with [`lower_for_job`] and [`restore`], instead of having been
/// lowered for life here.
#[cfg(windows)]
pub(super) fn begin_background() -> std::io::Result<bool> {
    Ok(true)
}

/// Prepares the calling background worker's priority as it starts. Returns whether the worker
/// lowers itself per job, with [`lower_for_job`] and [`restore`], instead of having been
/// lowered for life here. Android lowers per job when [`can_restore`] shows the worker may
/// raise its niceness back, and otherwise keeps the lifetime niceness that check left it at.
#[cfg(target_os = "android")]
pub(super) fn begin_background() -> std::io::Result<bool> {
    can_restore()
}

/// Prepares the calling background worker's priority as it starts. Returns whether the worker
/// lowers itself per job, with [`lower_for_job`] and [`restore`], instead of having been
/// lowered for life here.
#[cfg(not(any(windows, target_os = "android")))]
pub(super) fn begin_background() -> std::io::Result<bool> {
    lower().map(|()| false)
}

/// Lowers lighting workers relative to interactive frame and network work.
#[cfg(target_os = "macos")]
pub(super) fn lower() -> std::io::Result<()> {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut relative = 0;
    // Keep the inherited QoS class and lower only relative scheduling priority.
    let status =
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status));
    }
    if matches!(class, libc::qos_class_t::QOS_CLASS_UNSPECIFIED) {
        class = libc::qos_class_t::QOS_CLASS_DEFAULT;
    }
    // A negative relative priority lowers this worker without elevating any inherited setting.
    let status = unsafe { libc::pthread_set_qos_class_self_np(class, relative.min(-1)) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status))
    }
}

/// Niceness of a background worker lowered for its whole life.
#[cfg(any(target_os = "linux", target_os = "android"))]
const LIFETIME_NICENESS: i32 = 1;
/// Android's `THREAD_PRIORITY_BACKGROUND`, held only while a worker runs a light or summary job.
#[cfg(any(target_os = "linux", target_os = "android"))]
const PER_JOB_NICENESS: i32 = 10;
/// The weakest niceness Linux has.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
const WEAKEST_NICENESS: i32 = 19;

#[cfg(any(target_os = "linux", target_os = "android"))]
thread_local! {
    /// The niceness [`restore`] returns this worker to after [`lower_for_job`].
    static NICENESS_BEFORE_JOB: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// The calling worker's niceness. Linux and Android niceness belongs to one task, not the
/// whole process.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn niceness() -> i32 {
    // SAFETY: gettid has no preconditions, and getpriority only reads the calling task.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) }
}

/// Sets the calling worker's niceness.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn set_niceness(niceness: i32) -> std::io::Result<()> {
    // SAFETY: gettid has no preconditions, and this changes only the calling task.
    let status =
        unsafe { libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, niceness) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Lowers the calling worker for life; an inherited lower priority stays lower.
#[cfg(target_os = "linux")]
pub(super) fn lower() -> std::io::Result<()> {
    set_niceness(niceness().max(LIFETIME_NICENESS))
}

/// Lowers the calling worker for one light or summary job; [`restore`] undoes it. An inherited
/// lower priority stays lower.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn lower_for_job() -> std::io::Result<()> {
    let before = niceness();
    set_niceness(before.max(PER_JOB_NICENESS))?;
    NICENESS_BEFORE_JOB.set(Some(before));
    Ok(())
}

/// Returns a worker lowered by [`lower_for_job`] to the niceness it had before, and before it
/// publishes a result; does nothing otherwise.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn restore() -> std::io::Result<()> {
    match NICENESS_BEFORE_JOB.take() {
        Some(before) => set_niceness(before),
        None => Ok(()),
    }
}

/// Whether the calling worker may raise its niceness back after lowering it, found by lowering
/// it one step, to at least the lifetime niceness, and raising it again. A worker that may not
/// is left lowered for life.
///
/// Linux lets a task without `CAP_SYS_NICE` raise its niceness only down to
/// `20 - RLIMIT_NICE`. Desktop sessions usually leave that limit at zero. Android's init sets
/// it to 40 for every process, which is also what lets the client's frame threads reach
/// negative niceness.
#[cfg(any(target_os = "android", all(test, target_os = "linux")))]
fn can_restore() -> std::io::Result<bool> {
    let before = niceness();
    let lowered = (before + 1).clamp(LIFETIME_NICENESS, WEAKEST_NICENESS);
    set_niceness(lowered)?;
    Ok(lowered > before && set_niceness(before).is_ok())
}

/// Windows keeps background lighting below the normal frame/network thread priority.
#[cfg(windows)]
pub(super) fn lower() -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // The pseudo-handle is valid for the current worker and must not be closed.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Lowers the calling worker for one light or summary job; [`restore`] undoes it.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) fn lower_for_job() -> std::io::Result<()> {
    lower()
}

/// Returns a worker lowered by [`lower_for_job`] to normal priority, before it touches state
/// the frame thread also locks.
#[cfg(windows)]
pub(super) fn restore() -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_NORMAL,
    };
    // The pseudo-handle is valid for the current worker and must not be closed.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_NORMAL) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Other targets keep background workers lowered for life; see [`begin_background`].
#[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
pub(super) fn restore() -> std::io::Result<()> {
    Ok(())
}

/// Other targets retain queue isolation and bounded worker counts without an OS priority hint.
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    windows
)))]
pub(super) fn lower() -> std::io::Result<()> {
    Ok(())
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, windows))]
pub(super) fn is_lowered() -> bool {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_NORMAL,
    };
    // The pseudo-handle is valid for the current thread and must not be closed.
    unsafe { GetThreadPriority(GetCurrentThread()) < THREAD_PRIORITY_NORMAL }
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
pub(super) fn is_lowered() -> bool {
    // These calls address the current task only.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) > 0 }
}

/// Whether the calling thread runs below normal priority, as [`lower`] leaves it.
#[cfg(all(test, target_os = "macos"))]
pub(super) fn is_lowered() -> bool {
    let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut relative = 0;
    // The output pointers stay valid throughout the call on this thread.
    let status =
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative) };
    status == 0 && relative < 0
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    /// Reads back the actual priority on an isolated thread so the test runner stays unchanged.
    #[test]
    fn lighting_priority_is_applied_to_the_worker() {
        std::thread::spawn(|| {
            let mut inherited = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut inherited_relative = 0;
            // These outputs remain live on this test worker throughout the call.
            assert_eq!(
                unsafe {
                    libc::pthread_get_qos_class_np(
                        libc::pthread_self(),
                        &mut inherited,
                        &mut inherited_relative,
                    )
                },
                0
            );
            super::lower().unwrap();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // The output pointers stay valid throughout the call on this worker.
            let status = unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
            };
            assert_eq!(status, 0);
            let expected = if matches!(inherited, libc::qos_class_t::QOS_CLASS_UNSPECIFIED) {
                libc::qos_class_t::QOS_CLASS_DEFAULT
            } else {
                inherited
            };
            assert_eq!(class as u32, expected as u32);
            assert!(relative <= inherited_relative);
            assert!(relative < 0);
        })
        .join()
        .unwrap();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    /// Runs `body` on its own thread, so the test runner's niceness stays unchanged.
    fn on_worker<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(body).join().unwrap()
    }

    /// Desktop Linux lowers a background worker once, for life: restoring changes nothing.
    #[test]
    fn desktop_linux_lowers_background_workers_for_life() {
        on_worker(|| {
            let before = niceness();
            assert!(!begin_background().unwrap());
            assert_eq!(niceness(), before.max(LIFETIME_NICENESS));
            restore().unwrap();
            assert_eq!(niceness(), before.max(LIFETIME_NICENESS));
        });
    }

    /// The check Android workers run at start either proves a worker can return to its
    /// niceness, leaving it there, or leaves it lowered for life.
    #[test]
    fn restore_check_leaves_a_worker_restored_or_lowered_for_life() {
        on_worker(|| {
            let before = niceness();
            if can_restore().unwrap() {
                assert_eq!(niceness(), before);
            } else {
                assert!(niceness() >= before.max(LIFETIME_NICENESS));
                assert!(is_lowered());
            }
        });
    }

    /// Where the host lets a worker raise its niceness, a job runs at background niceness and
    /// the worker returns to its own afterwards; a second restore changes nothing.
    #[test]
    fn per_job_lowering_returns_to_the_previous_niceness() {
        on_worker(|| {
            let before = niceness();
            if !can_restore().unwrap() {
                eprintln!("skipping: missing fixture: an RLIMIT_NICE that lets niceness rise");
                return;
            }
            lower_for_job().unwrap();
            assert_eq!(niceness(), before.max(PER_JOB_NICENESS));
            restore().unwrap();
            assert_eq!(niceness(), before);
            restore().unwrap();
            assert_eq!(niceness(), before);
        });
    }
}
