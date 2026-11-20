#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod entry;

#[cfg(all(
    feature = "mimalloc",
    not(feature = "alloc-stats"),
    not(feature = "heap-profile")
))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(all(
    feature = "alloc-stats",
    feature = "mimalloc",
    not(feature = "heap-profile")
))]
#[global_allocator]
static ALLOCATOR: test_alloc::TrackingAllocator<mimalloc::MiMalloc> =
    test_alloc::TrackingAllocator(mimalloc::MiMalloc);

#[cfg(all(
    feature = "alloc-stats",
    not(feature = "mimalloc"),
    not(feature = "heap-profile")
))]
#[global_allocator]
static ALLOCATOR: test_alloc::TrackingAllocator<std::alloc::System> =
    test_alloc::TrackingAllocator(std::alloc::System);

#[cfg(all(feature = "heap-profile", target_os = "linux", target_env = "gnu"))]
#[global_allocator]
static ALLOCATOR: test_alloc::heap_profile::ProfilingAllocator<std::alloc::System> =
    test_alloc::heap_profile::ProfilingAllocator(std::alloc::System);

/// Runs the client through its shared startup and exit dispatch.
fn main() {
    #[cfg(all(feature = "heap-profile", target_os = "linux", target_env = "gnu"))]
    test_alloc::heap_profile::start();
    cinnabar::lifecycle::record_launch();
    entry::dispatch();
}
