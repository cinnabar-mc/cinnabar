//! The client's own releases under host memory pressure, and the Android edge that reports it.
//!
//! Audio and upload staging register their releases with their owners; this module publishes
//! the pressure, releases the resource packs' read caches and returns freed heap to the OS.

use bevy::prelude::{App, Last, MessageReader, Res, Update};
use memory_pressure::{MemoryPressure, MemoryPressurePlugin};

use crate::runtime::network::PackReload;

/// Installs pressure publication, the pack read-cache release and the heap purge.
pub(crate) fn configure(app: &mut App) {
    app.add_plugins(MemoryPressurePlugin)
        .add_systems(Update, release_pack_read_caches)
        .add_systems(Last, purge_released_heap);
}

/// Reports an Android `onTrimMemory` level, from the activity's thread.
///
/// Levels for a hidden process arrive after the client's last frame has emptied its caches, and
/// no later frame runs until it returns, so those levels return the freed heap to the OS here.
pub fn report_android_trim_level(level: i32) {
    let Some(pressure) = MemoryPressure::from_android_trim_level(level) else {
        return;
    };
    memory_pressure::report(pressure);
    if pressure == MemoryPressure::Background {
        purge_heap(pressure);
    }
}

/// Releases the inflated files that resource-pack stacks cache beside their archives. The
/// archives stay, and a reload or join that needs a file inflates it again.
fn release_pack_read_caches(
    mut pressure: MessageReader<MemoryPressure>,
    reload: Option<Res<PackReload>>,
) {
    let Some(pressure) = pressure.read().copied().max() else {
        return;
    };
    if let Some(reload) = reload {
        // The reads are dropped off the frame, after this frame's own purge has run.
        client_runtime::session::release_off_frame(DropThen {
            released: Some(reload.take_cached_reads()),
            then: Some(move || purge_emptied_heap(pressure)),
        });
    }
}

/// Carries released allocations to the thread that drops them and runs `then` once they are
/// freed. A purge that ran before the drop would find them still allocated.
struct DropThen<T, F: FnOnce()> {
    released: Option<T>,
    then: Option<F>,
}

impl<T, F: FnOnce()> Drop for DropThen<T, F> {
    fn drop(&mut self) {
        drop(self.released.take());
        if let Some(then) = self.then.take() {
            then();
        }
    }
}

/// Returns freed heap pages to the OS at the end of a frame whose pressure emptied caches.
fn purge_released_heap(mut pressure: MessageReader<MemoryPressure>) {
    if let Some(pressure) = pressure.read().copied().max() {
        purge_emptied_heap(pressure);
    }
}

/// Purges the heap after a release that emptied caches. Low pressure only shrinks them, which
/// is not worth a purge's pause on a visible client.
fn purge_emptied_heap(pressure: MemoryPressure) {
    if pressure.releases_everything() {
        purge_heap(pressure);
    }
}

/// Asks the system allocator to return unused pages to the OS. Only Android's allocator takes
/// the request; elsewhere this does nothing.
fn purge_heap(pressure: MemoryPressure) {
    #[cfg(target_os = "android")]
    android::purge(pressure == MemoryPressure::Background);
    #[cfg(not(target_os = "android"))]
    let _ = pressure;
}

#[cfg(target_os = "android")]
mod android {
    use std::ffi::c_int;

    /// bionic `M_PURGE`, since API 28: returns memory not in use to the kernel.
    const M_PURGE: c_int = -101;
    /// bionic `M_PURGE_ALL`, since API 34: a slower purge that examines every allocation cache.
    const M_PURGE_ALL: c_int = -104;

    unsafe extern "C" {
        /// bionic `mallopt`, since API 26. Returns 1 on success and 0 for an option this
        /// Android version does not know.
        fn mallopt(option: c_int, value: c_int) -> c_int;
    }

    /// Purges unused heap. A hidden client takes the thorough purge where the device has it; a
    /// visible one takes the quick purge, which older devices also fall back to.
    pub(super) fn purge(thorough: bool) {
        // SAFETY: `mallopt` takes two integers, is thread-safe, and only fails for an option the
        // running Android version lacks.
        unsafe {
            if !thorough || mallopt(M_PURGE_ALL, 0) == 0 {
                mallopt(M_PURGE, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The purge that follows an off-frame release must find the released memory already freed.
    #[test]
    fn the_follow_up_runs_only_after_the_released_allocations_are_dropped() {
        struct Released<'a>(&'a AtomicBool);
        impl Drop for Released<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let (freed, freed_first) = (AtomicBool::new(false), AtomicBool::new(false));
        let carried = DropThen {
            released: Some(Released(&freed)),
            then: Some(|| freed_first.store(freed.load(Ordering::Acquire), Ordering::Release)),
        };
        assert!(!freed.load(Ordering::Acquire));
        drop(carried);
        assert!(freed_first.load(Ordering::Acquire));
    }
}
