//! Join-time compile results persisted across launches, under one bound.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};

/// Server-pack archives and compiled join results persist here; unset, nothing is written.
static COMPILE_CACHE: std::sync::OnceLock<CompileCache> = std::sync::OnceLock::new();

/// Bounds the persisted results; Zeqa's compiled entity pack is about 150 MB.
const COMPILE_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Selects the persistent cache directory once for the current process.
pub fn set_compile_cache_dir(dir: std::path::PathBuf) {
    let _ = COMPILE_CACHE.set(CompileCache::new(dir, COMPILE_CACHE_BYTES));
}

/// Returns the configured cache, or None when persistent caching is disabled.
pub fn current() -> Option<&'static CompileCache> {
    COMPILE_CACHE.get()
}

const MAGIC: [u8; 8] = *b"CNBRCMP3";
const HASH: usize = 32;
const KEY_OFFSET: usize = MAGIC.len();
const DECODED_LENGTH_OFFSET: usize = KEY_OFFSET + HASH;
const HEADER: usize = DECODED_LENGTH_OFFSET + std::mem::size_of::<u64>();
/// The compressed length and the digest of everything before it follow the payload, so an
/// entry is written and verified in one streaming pass.
const TRAILER: usize = std::mem::size_of::<u64>() + HASH;
const EXTENSION: &str = "bin";
/// A writer that died mid-entry leaves its temporary file; later writers sweep it after this.
const STALE_TEMPORARY: Duration = Duration::from_secs(3600);

/// A size-bounded on-disk store of compiled subscriber payloads, least recently used first out.
#[derive(Clone, Debug)]
pub struct CompileCache {
    dir: PathBuf,
    max_bytes: u64,
}

/// Starts a cache key that also names the build, so a new client never reads an older layout.
pub fn cache_key(subscriber: &str) -> Sha256 {
    let mut key = Sha256::new();
    part(&mut key, env!("CARGO_PKG_VERSION").as_bytes());
    part(&mut key, &build_identity());
    part(&mut key, subscriber.as_bytes());
    key
}

/// Length framing keeps adjacent key parts from aliasing.
pub fn part(key: &mut Sha256, bytes: &[u8]) {
    key.update((bytes.len() as u64).to_le_bytes());
    key.update(bytes);
}

/// Size and modification time of the running executable; any rebuild changes it.
fn build_identity() -> [u8; 16] {
    static IDENTITY: std::sync::OnceLock<[u8; 16]> = std::sync::OnceLock::new();
    *IDENTITY.get_or_init(|| {
        let metadata = std::env::current_exe().and_then(fs::metadata).ok();
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos() as u64);
        let length = metadata.map_or(0, |metadata| metadata.len());
        let mut identity = [0; 16];
        identity[..8].copy_from_slice(&length.to_le_bytes());
        identity[8..].copy_from_slice(&modified.to_le_bytes());
        identity
    })
}

impl CompileCache {
    #[must_use]
    pub fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self { dir, max_bytes }
    }

    /// Reuses a subscriber output and its reads, compiling on a miss or invalid entry.
    pub fn get_or_compile<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        key: &[u8; 32],
        view: &resource_pack::LayeredPackView,
        compile: impl FnOnce() -> T,
    ) -> T {
        let Some(files) = view.dependencies() else {
            return compile();
        };
        if let Some(entry) = self.load(key)
            && let Some((dependencies, payload)) = resource_pack::PackDependency::decode_set(&entry)
            && let Ok((output, [])) = postcard::take_from_bytes(payload)
        {
            files.extend(dependencies);
            return output;
        }
        let output = compile();
        if let Ok(payload) = postcard::to_allocvec(&output) {
            let mut reads = Vec::new();
            resource_pack::PackDependency::encode_set(&files.snapshot(), &mut reads);
            self.store_with(key, (reads.len() + payload.len()) as u64, |entry| {
                entry.write_all(&reads)?;
                entry.write_all(&payload)
            });
        }
        output
    }

    /// Returns the stored payload, or `None` for a miss; a damaged entry is deleted.
    pub fn load(&self, key: &[u8; 32]) -> Option<Vec<u8>> {
        self.load_with(key, |payload, length| {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(length).ok()?)
                .ok()?;
            payload.read_to_end(&mut bytes).ok()?;
            Some(bytes)
        })
    }

    /// Decompresses the stored payload straight into `read`, which receives a reader of the
    /// payload and its stored length, so no compressed or decompressed copy is staged.
    /// The entry is verified once `read` returns: a damaged entry is deleted and whatever
    /// `read` built from it is dropped. `None` for a miss or when `read` declines.
    pub fn load_with<T>(
        &self,
        key: &[u8; 32],
        read: impl FnOnce(&mut dyn Read, u64) -> Option<T>,
    ) -> Option<T> {
        let path = self.entry(key);
        let file = fs::File::open(&path).ok()?;
        let physical = file.metadata().ok()?.len();
        let mut file = std::io::BufReader::new(file);
        let mut header = [0; HEADER];
        let length = (physical <= self.max_bytes
            && physical >= (HEADER + TRAILER) as u64
            && file.read_exact(&mut header).is_ok()
            && header[..KEY_OFFSET] == MAGIC
            && header[KEY_OFFSET..DECODED_LENGTH_OFFSET] == key[..])
            .then(|| u64::from_le_bytes(header[DECODED_LENGTH_OFFSET..].try_into().unwrap()))
            .filter(|length| *length <= self.max_bytes);
        let Some(length) = length else {
            let _ = fs::remove_file(&path);
            return None;
        };
        let compressed = physical - (HEADER + TRAILER) as u64;
        let mut body = DigestingReader {
            inner: (&mut file).take(compressed),
            digest: Sha256::new_with_prefix(header),
        };
        let mut decoder = flate2::read::DeflateDecoder::new(&mut body);
        let mut payload = (&mut decoder).take(length);
        let output = read(&mut payload, length);
        // The payload must end exactly at its stored length, and the stream with it.
        let complete = std::io::copy(&mut payload, &mut std::io::sink()).is_ok()
            && payload.limit() == 0
            && matches!(decoder.read(&mut [0]), Ok(0))
            && decoder.total_in() == compressed;
        drop(decoder);
        let DigestingReader { digest, .. } = body;
        let mut trailer = [0; TRAILER];
        let intact = complete
            && file.read_exact(&mut trailer).is_ok()
            && trailer[..TRAILER - HASH] == compressed.to_le_bytes()
            && digest.chain_update(&trailer[..TRAILER - HASH]).finalize()[..]
                == trailer[TRAILER - HASH..];
        if !intact {
            tracing::warn!(path = %path.display(), "discarding a damaged compile cache entry");
            let _ = fs::remove_file(&path);
            return None;
        }
        // Recency for eviction; a read-only cache still serves the hit.
        let _ = fs::File::options()
            .append(true)
            .open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()));
        output
    }

    /// Stores `payload` as [`Self::store_with`] does.
    pub fn store(&self, key: &[u8; 32], payload: &[u8]) {
        self.store_with(key, payload.len() as u64, |entry| entry.write_all(payload));
    }

    /// Streams exactly `length` payload bytes from `write` through compression into the entry
    /// for `key`, then evicts the least recently used entries past the size bound. Nothing is
    /// stored when the payload or its entry exceeds the bound, or when `write` fails or
    /// writes another length.
    pub fn store_with(
        &self,
        key: &[u8; 32],
        length: u64,
        write: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) {
        if length > self.max_bytes {
            return;
        }
        match self.write(key, length, write) {
            Ok(()) => self.evict(&self.entry(key)),
            Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {}
            Err(error) => {
                tracing::warn!(%error, dir = %self.dir.display(), "compile cache entry was not stored");
            }
        }
    }

    fn entry(&self, key: &[u8; 32]) -> PathBuf {
        let name: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        self.dir.join(name).with_extension(EXTENSION)
    }

    /// No fsync: a torn entry fails validation on load and is treated as a miss.
    fn write(
        &self,
        key: &[u8; 32],
        length: u64,
        write: impl FnOnce(&mut dyn Write) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        fs::create_dir_all(&self.dir)?;
        let path = self.entry(key);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}.{sequence}.tmp", std::process::id()));
        let written = (|| {
            let mut entry = DigestingWriter {
                inner: std::io::BufWriter::new(fs::File::create(&temporary)?),
                digest: Sha256::new(),
                written: 0,
                limit: self.max_bytes.saturating_sub(HASH as u64),
            };
            for part in [&MAGIC[..], key, &length.to_le_bytes()] {
                entry.write_all(part)?;
            }
            let mut encoder =
                flate2::write::DeflateEncoder::new(entry, flate2::Compression::fast());
            let mut payload = DeclaredPayload {
                inner: &mut encoder,
                remaining: length,
            };
            write(&mut payload)?;
            if payload.remaining != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "compile cache payload is shorter than declared",
                ));
            }
            let mut entry = encoder.finish()?;
            let compressed = entry.written - HEADER as u64;
            entry.write_all(&compressed.to_le_bytes())?;
            let DigestingWriter {
                mut inner, digest, ..
            } = entry;
            inner.write_all(&digest.finalize())?;
            inner
                .into_inner()
                .map_err(std::io::IntoInnerError::into_error)?;
            Ok(())
        })();
        let renamed = written.and_then(|()| fs::rename(&temporary, &path));
        if renamed.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        renamed
    }

    fn evict(&self, keep: &Path) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        let mut entries: Vec<_> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if path.extension().is_some_and(|extension| extension == "tmp") {
                    if now.duration_since(modified).unwrap_or_default() > STALE_TEMPORARY {
                        let _ = fs::remove_file(&path);
                    }
                    return None;
                }
                (metadata.is_file() && path.extension().is_some_and(|ext| ext == EXTENSION))
                    .then_some((modified, metadata.len(), path))
            })
            .collect();
        let mut total: u64 = entries.iter().map(|(_, length, _)| length).sum();
        entries.sort();
        for (_, length, path) in entries {
            if total <= self.max_bytes {
                break;
            }
            if path != keep && fs::remove_file(&path).is_ok() {
                total -= length;
            }
        }
    }
}

/// Digests and counts the bytes that pass through, refusing to grow an entry past `limit`.
struct DigestingWriter<W> {
    inner: W,
    digest: Sha256,
    written: u64,
    limit: u64,
}

impl<W: Write> Write for DigestingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written + bytes.len() as u64 > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "compile cache entry exceeds its bound",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.digest.update(&bytes[..written]);
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Accepts at most the payload length declared in the entry header.
struct DeclaredPayload<W> {
    inner: W,
    remaining: u64,
}

impl<W: Write> Write for DeclaredPayload<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "compile cache payload is longer than declared",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Digests every byte read through it.
struct DigestingReader<R> {
    inner: R,
    digest: Sha256,
}

impl<R: Read> Read for DigestingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.digest.update(&buffer[..read]);
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn age(cache: &CompileCache, key: &[u8; 32], seconds: u64) {
        fs::File::options()
            .append(true)
            .open(cache.entry(key))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn a_stored_entry_loads_back_and_other_keys_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().join("compiled"), 1 << 20);
        cache.store(&key(1), b"payload");
        assert_eq!(cache.load(&key(1)).as_deref(), Some(&b"payload"[..]));
        assert_eq!(cache.load(&key(2)), None);
    }

    #[test]
    fn a_damaged_entry_is_a_miss_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        for damage in [
            (|bytes: &mut Vec<u8>| bytes[50] ^= 1) as fn(&mut Vec<u8>),
            |bytes| bytes.truncate(bytes.len() - 1),
            |bytes| bytes.truncate(3),
        ] {
            cache.store(&key(1), b"payload bytes");
            let path = cache.entry(&key(1));
            let mut bytes = fs::read(&path).unwrap();
            damage(&mut bytes);
            fs::write(&path, bytes).unwrap();
            assert_eq!(cache.load(&key(1)), None);
            assert!(!path.exists());
        }
    }

    #[test]
    fn an_entry_renamed_to_another_key_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        cache.store(&key(1), b"payload");
        fs::rename(cache.entry(&key(1)), cache.entry(&key(2))).unwrap();
        assert_eq!(cache.load(&key(2)), None);
    }

    #[test]
    fn eviction_drops_the_least_recently_used_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        cache.store(&key(1), &[1; 100]);
        cache.store(&key(2), &[2; 100]);
        cache.max_bytes = fs::metadata(cache.entry(&key(1))).unwrap().len()
            + fs::metadata(cache.entry(&key(2))).unwrap().len();
        age(&cache, &key(1), 20);
        age(&cache, &key(2), 10);
        // A hit refreshes the older entry, so the other one is evicted.
        assert!(cache.load(&key(1)).is_some());
        cache.store(&key(3), &[3; 100]);
        assert!(cache.load(&key(1)).is_some());
        assert_eq!(cache.load(&key(2)), None);
        assert!(cache.load(&key(3)).is_some());
    }

    #[test]
    fn an_entry_larger_than_the_bound_is_not_stored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 64);
        cache.store(&key(1), &[0; 64]);
        assert_eq!(cache.load(&key(1)), None);
    }

    #[test]
    fn keys_change_with_each_framed_part() {
        let digest = |parts: &[&[u8]]| {
            let mut key = cache_key("entities");
            for bytes in parts {
                part(&mut key, bytes);
            }
            key.finalize()
        };
        assert_ne!(digest(&[b"ab", b"c"]), digest(&[b"a", b"bc"]));
        assert_ne!(
            cache_key("entities").finalize(),
            cache_key("glyphs").finalize()
        );
    }
}

#[cfg(test)]
#[path = "compile_cache/tests.rs"]
mod subscriber_tests;
