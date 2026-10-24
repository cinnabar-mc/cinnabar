//! Atomically publishes complete cache objects without replacing existing destinations.

use cap_std::fs::Dir;
use std::io;

/// Identifies an occupied final path without following or replacing it.
pub(super) fn collision() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "pack cache destination already exists",
    )
}

/// Avoids hard links on Android filesystems while retaining atomic no-replace publication.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn publish(dir: &Dir, source: &str, destination: &str) -> io::Result<()> {
    rustix::fs::renameat_with(
        dir,
        source,
        dir,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)
}

/// Creates the final name atomically; the caller's temporary guard handles any cleanup failure.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub(super) fn publish(dir: &Dir, source: &str, destination: &str) -> io::Result<()> {
    dir.hard_link(source, dir, destination)?;
    dir.remove_file(source)
}
