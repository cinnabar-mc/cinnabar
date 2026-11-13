//! Linux and Android CPU affinity, and the threads confined to the background CPUs.

use std::sync::{Mutex, PoisonError};

/// The process's confined background threads.
pub(crate) static BACKGROUND: ConfinedThreads = ConfinedThreads::new();

/// Live threads confined to a CPU set, by kernel task ID, so the confinement can be repeated
/// after the system resets it. A thread leaves the list when it exits.
pub(crate) struct ConfinedThreads {
    tasks: Mutex<Vec<libc::pid_t>>,
}

/// Removes its thread from the list it joined when the thread exits, so a task ID the kernel
/// reuses for a frame thread is never confined.
struct Membership {
    threads: &'static ConfinedThreads,
    task: libc::pid_t,
}

impl Drop for Membership {
    fn drop(&mut self) {
        // Holding the lock here keeps the task ID alive for a refresh in progress.
        self.threads.lock().retain(|&task| task != self.task);
    }
}

thread_local! {
    /// The calling thread's place in a [`ConfinedThreads`] list, held until the thread exits.
    static MEMBERSHIP: std::cell::OnceCell<Membership> = const { std::cell::OnceCell::new() };
}

impl ConfinedThreads {
    pub(crate) const fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<libc::pid_t>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lets the calling thread run only on `cpus` and lists it for
    /// [`Self::confine_registered_threads`], even when the kernel refuses now. A thread joins
    /// only the first list it is confined through.
    pub(crate) fn confine_current_thread(&'static self, cpus: &[usize]) -> std::io::Result<()> {
        // SAFETY: gettid has no preconditions.
        let task = unsafe { libc::gettid() };
        MEMBERSHIP.with(|membership| {
            membership.get_or_init(|| {
                self.lock().push(task);
                Membership {
                    threads: self,
                    task,
                }
            });
        });
        confine(task, cpus)
    }

    /// Confines every listed thread to `cpus` again, ignoring threads the kernel refuses.
    pub(crate) fn confine_registered_threads(&self, cpus: &[usize]) {
        for &task in self.lock().iter() {
            let _ = confine(task, cpus);
        }
    }
}

/// The CPUs the calling thread may run on, ascending; `None` if the kernel will not say.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn current_cpus() -> Option<Vec<usize>> {
    // SAFETY: An all-zero cpu_set_t is a valid empty set.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: Thread ID zero addresses the calling thread, and `set` outlives the call.
    let status =
        unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) };
    if status != 0 {
        return None;
    }
    let cpus: Vec<usize> = (0..set_capacity())
        // SAFETY: Every index is below the set's capacity.
        .filter(|&cpu| unsafe { libc::CPU_ISSET(cpu, &set) })
        .collect();
    (!cpus.is_empty()).then_some(cpus)
}

/// Lets thread `task` of this process run only on `cpus`.
fn confine(task: libc::pid_t, cpus: &[usize]) -> std::io::Result<()> {
    // SAFETY: An all-zero cpu_set_t is a valid empty set.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for &cpu in cpus.iter().filter(|&&cpu| cpu < set_capacity()) {
        // SAFETY: The filter keeps every index below the set's capacity.
        unsafe { libc::CPU_SET(cpu, &mut set) };
    }
    // SAFETY: `set` outlives the call, which changes only the named thread's affinity.
    let status =
        unsafe { libc::sched_setaffinity(task, std::mem::size_of::<libc::cpu_set_t>(), &set) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// CPU numbers one `cpu_set_t` can hold.
const fn set_capacity() -> usize {
    std::mem::size_of::<libc::cpu_set_t>() * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `body` on a fresh thread, returning its result once the thread has exited.
    fn on_thread<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(body).join().unwrap()
    }

    /// A confined thread runs only on the given CPUs, and so do threads it spawns.
    #[test]
    fn confinement_applies_to_the_thread_and_its_children() {
        static THREADS: ConfinedThreads = ConfinedThreads::new();
        let Some(allowed) = current_cpus().filter(|cpus| cpus.len() >= 2) else {
            eprintln!("skipping: missing fixture: a second CPU in the test's affinity mask");
            return;
        };
        let target = vec![allowed[0]];
        let (own, child) = on_thread({
            let target = target.clone();
            move || {
                THREADS.confine_current_thread(&target).unwrap();
                (current_cpus(), on_thread(current_cpus))
            }
        });
        assert_eq!(own.as_deref(), Some(&target[..]));
        assert_eq!(child.as_deref(), Some(&target[..]));
    }

    /// A refresh restores confinement the system reset, and skips threads that have exited.
    #[test]
    fn refresh_confines_live_threads_again_and_forgets_exited_ones() {
        static THREADS: ConfinedThreads = ConfinedThreads::new();
        let Some(allowed) = current_cpus().filter(|cpus| cpus.len() >= 2) else {
            eprintln!("skipping: missing fixture: a second CPU in the test's affinity mask");
            return;
        };
        let target = vec![allowed[0]];
        let (confined_tx, confined_rx) = std::sync::mpsc::channel();
        let (refreshed_tx, refreshed_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn({
            let (target, allowed) = (target.clone(), allowed.clone());
            move || {
                THREADS.confine_current_thread(&target).unwrap();
                // Stands in for a cpuset move that hands the thread every CPU again.
                // SAFETY: gettid has no preconditions.
                confine(unsafe { libc::gettid() }, &allowed).unwrap();
                confined_tx.send(current_cpus()).unwrap();
                refreshed_rx.recv().unwrap();
                current_cpus()
            }
        });
        assert_eq!(confined_rx.recv().unwrap(), Some(allowed));
        THREADS.confine_registered_threads(&target);
        refreshed_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Some(target.clone()));
        assert!(THREADS.lock().is_empty(), "an exited thread stays listed");
        // Nothing is listed, so nothing changes.
        THREADS.confine_registered_threads(&target);
    }
}
