//! Immutable source bytes with immediate metadata checks and periodic content revalidation.

use super::{Opened, open};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    path::Path,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Rehash unchanged sources every two seconds: four normal 500 ms polls share one read.
/// Metadata alone cannot detect every write, including writes through existing mappings.
pub const CONTENT_REVALIDATION_INTERVAL: Duration = Duration::from_secs(2);

/// Bounds how long unchanged metadata may stand in for a fresh content read.
pub(crate) struct ContentDeadline(Instant);

impl ContentDeadline {
    /// Starts the next content revalidation window after a bounded read.
    pub(crate) fn new(now: Instant) -> Self {
        Self(now + CONTENT_REVALIDATION_INTERVAL)
    }

    /// Allows metadata-only reuse strictly before the next content revalidation.
    pub(crate) fn allows_reuse(&self, now: Instant) -> bool {
        now < self.0
    }
}

/// One bounded source read and its digest, shared without copying the source buffer.
#[derive(Clone)]
pub struct Snapshot {
    pub bytes: Arc<[u8]>,
    pub digest: [u8; 32],
}

struct Cached {
    identity: Identity,
    snapshot: Snapshot,
    observed: bool,
    deadline: ContentDeadline,
}

/// A worker-owned cache; each poll still reopens paths with the regular-file/link policy.
/// Equal metadata permits reuse only until the content revalidation deadline.
/// Finish each bounded source poll with `retain_observed`, including empty polls.
#[derive(Default)]
pub struct ObservationCache {
    files: BTreeMap<(PathBuf, usize), Cached>,
}

impl ObservationCache {
    /// Reads and hashes changed files, or shares the prior bytes after checking the opened leaf.
    pub fn read(&mut self, path: &Path, limit: usize, now: Instant) -> io::Result<Snapshot> {
        self.read_at(path, limit, now, Identity::of, |opened| opened.read(limit))
    }

    /// Drops files absent from the completed poll and starts tracking the next poll's reads.
    pub fn retain_observed(&mut self) {
        self.files.retain(|_, cached| {
            let observed = cached.observed;
            cached.observed = false;
            observed
        });
    }

    /// Revalidates content at each deadline even when metadata never changes.
    fn read_at(
        &mut self,
        path: &Path,
        limit: usize,
        now: Instant,
        identify: impl Fn(&File) -> io::Result<Identity>,
        read: impl FnOnce(&mut Opened) -> io::Result<Vec<u8>>,
    ) -> io::Result<Snapshot> {
        let key = (path.to_owned(), limit);
        let mut opened = open(path)?;
        let before = identify(&opened.file)?;
        if let Some(cached) = self.files.get_mut(&key)
            && before == cached.identity
            && cached.deadline.allows_reuse(now)
        {
            cached.observed = true;
            return Ok(cached.snapshot.clone());
        }
        let previous = self.files.remove(&key);
        if opened.file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("source exceeds byte limit"));
        }
        let bytes = read(&mut opened)?;
        let after = identify(&opened.file)?;
        if before != after {
            return Err(io::Error::other("source changed while being read"));
        }
        let digest = Sha256::digest(&bytes).into();
        let snapshot = match previous {
            Some(previous) if previous.snapshot.digest == digest => previous.snapshot,
            _ => Snapshot {
                digest,
                bytes: bytes.into(),
            },
        };
        self.files.insert(
            key,
            Cached {
                identity: after,
                snapshot: snapshot.clone(),
                observed: true,
                deadline: ContentDeadline::new(now),
            },
        );
        Ok(snapshot)
    }
}

/// Volume and file number; Windows tunnels a replaced name's creation time, but not this.
type FileNumber = (u64, u64);
/// Metadata change time, when the file system reports one.
type ChangeTime = Option<(i64, i64)>;

/// File identity plus content and metadata change times; access times are deliberately excluded.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    file: FileNumber,
    created: Option<std::time::SystemTime>,
    modified: Option<std::time::SystemTime>,
    changed: ChangeTime,
    len: u64,
}

impl Identity {
    /// Captures best-effort change metadata; the deadline covers writes it cannot distinguish.
    pub(crate) fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        #[cfg(unix)]
        let (file, changed) = {
            use std::os::unix::fs::MetadataExt;
            (
                (metadata.dev(), metadata.ino()),
                Some((metadata.ctime(), metadata.ctime_nsec())),
            )
        };
        #[cfg(windows)]
        let (file, changed) = windows_identity(file)?;
        #[cfg(not(any(unix, windows)))]
        let (file, changed) = ((0, 0), None);
        Ok(Self {
            file,
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
            changed,
            len: metadata.len(),
        })
    }
}

/// Returns the volume and file number, plus the change time when the file system reports one.
#[cfg(windows)]
fn windows_identity(file: &File) -> io::Result<(FileNumber, ChangeTime)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle stays open for this call, which writes only the provided struct.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: the buffer is a FILE_BASIC_INFO of the size passed, as FileBasicInfo requires.
    let changed = (unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&raw mut basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } != 0)
        .then_some((basic.ChangeTime, 0));
    Ok(((u64::from(info.dwVolumeSerialNumber), index), changed))
}

#[cfg(test)]
mod tests;
