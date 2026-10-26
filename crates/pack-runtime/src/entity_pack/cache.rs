use assets::SessionEntityPack;
use resource_pack::LayeredPackView;
use resource_pack::StackFingerprint;
use std::{io::Read, path::PathBuf, sync::Arc};

#[path = "pixel_reuse.rs"]
mod pixel_reuse;

/// Pack content, reference definitions, and the startup artwork paths a compile reads.
#[derive(Clone)]
pub(super) struct EntityInputs {
    pub(super) stack: StackFingerprint,
    pub(super) vanilla: Option<Arc<assets::VanillaEntityRefs>>,
    pub(super) vanilla_pack_dir: Option<PathBuf>,
    pub(super) equipment: Option<Arc<assets::RuntimeEquipmentCatalog>>,
}

impl PartialEq for EntityInputs {
    /// Loaded texture-path identities keep rebuilt artwork tables out of stale cache entries.
    fn eq(&self, other: &Self) -> bool {
        let Self {
            stack,
            vanilla,
            vanilla_pack_dir,
            equipment,
        } = self;
        stack == &other.stack
            && vanilla == &other.vanilla
            && vanilla_pack_dir == &other.vanilla_pack_dir
            && equipment
                .as_ref()
                .map(|catalog| catalog.texture_paths_sha256())
                == other
                    .equipment
                    .as_ref()
                    .map(|catalog| catalog.texture_paths_sha256())
    }
}

/// A compile's result, held weakly so a pack outlives its session only while a kept stack owns it;
/// `None` when the stack defined no entity.
type Retained = Option<std::sync::Weak<SessionEntityPack>>;

type CachedEntities = (
    EntityInputs,
    Retained,
    Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
);

/// The retained result, or `None` once every owner has released the pack.
fn upgrade(retained: &Retained) -> Option<Option<Arc<SessionEntityPack>>> {
    match retained {
        None => Some(None),
        Some(pack) => pack.upgrade().map(Some),
    }
}

/// A compiled pack and, when asked for, the entity blob it was built from.
pub(super) type Compiled = (Option<Arc<SessionEntityPack>>, Option<Box<[u8]>>);

/// Runs the encode and write of a fresh compile's disk entry.
type StoreEntry = fn(Box<dyn FnOnce() + Send>);

/// The two latest join and composed reload compiles, reused across reconnects while owned.
pub(super) struct EntityCache {
    last: std::sync::Mutex<std::collections::VecDeque<CachedEntities>>,
    store: StoreEntry,
}

impl Default for EntityCache {
    /// Writes disk entries before a compile returns.
    fn default() -> Self {
        Self {
            last: std::sync::Mutex::new(std::collections::VecDeque::new()),
            store: |write| write(),
        }
    }
}

/// Background disk-cache writes queued or running.
static PENDING_STORES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether every queued background store has finished, so offline reports can include its cost.
#[cfg(any(test, feature = "test-support"))]
pub fn stores_idle() -> bool {
    PENDING_STORES.load(std::sync::atomic::Ordering::Acquire) == 0
}

/// Writes optional disk-cache entries on one worker, with room for one waiting entry.
/// Overflow or incomplete writes leave a cache miss; joins never wait for disk encoding.
fn store_in_background(write: Box<dyn FnOnce() + Send>) {
    use std::sync::atomic::Ordering;
    type Writes = std::sync::Mutex<std::sync::mpsc::SyncSender<Box<dyn FnOnce() + Send>>>;
    static WRITER: std::sync::OnceLock<Option<Writes>> = std::sync::OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (writes, pending) = std::sync::mpsc::sync_channel::<Box<dyn FnOnce() + Send>>(1);
        std::thread::Builder::new()
            .name("entity-pack-store".to_owned())
            .spawn(move || pending.into_iter().for_each(|write| write()))
            .map_err(|error| tracing::debug!(%error, "compiled entity packs are not stored"))
            .ok()
            .map(|_| std::sync::Mutex::new(writes))
    });
    let Some(writer) = writer else {
        return;
    };
    PENDING_STORES.fetch_add(1, Ordering::AcqRel);
    let queued = writer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .try_send(Box::new(move || {
            write();
            PENDING_STORES.fetch_sub(1, Ordering::AcqRel);
        }));
    if queued.is_err() {
        PENDING_STORES.fetch_sub(1, Ordering::AcqRel);
        tracing::debug!("a compiled entity pack was not stored while others were");
    }
}

impl EntityCache {
    /// Returns the cached pack for `inputs`, then a disk hit, compiling without holding the lock.
    /// `None` once `cancelled` stopped the compile, which then records and stores nothing.
    pub(super) fn get_or_compile(
        &self,
        inputs: EntityInputs,
        view: &LayeredPackView,
        disk: Option<&crate::compile_cache::CompileCache>,
        cancelled: &dyn Fn() -> bool,
        compile: impl FnOnce(
            &LayeredPackView,
            Option<&assets::VanillaEntityRefs>,
            Option<&std::path::Path>,
            Option<&assets::RuntimeEquipmentCatalog>,
            bool,
            &dyn Fn() -> bool,
        ) -> Option<Compiled>,
    ) -> Option<Option<Arc<SessionEntityPack>>> {
        {
            let mut cache = self.lock();
            cache.retain(|(_, pack, _)| upgrade(pack).is_some());
            if let Some(index) = cache.iter().position(|(cached, _, files)| {
                *cached == inputs && (view.dependencies().is_none() || files.is_some())
            }) {
                let entry = cache.remove(index).expect("a matched entry is present");
                if let (Some(dependencies), Some(files)) = (view.dependencies(), &entry.2) {
                    dependencies.extend(files.clone());
                }
                if let Some(pack) = upgrade(&entry.1) {
                    cache.push_front(entry);
                    return Some(pack);
                }
            }
        }
        // Only a tracked compile knows its reads, which a reload diff of a disk hit needs.
        let disk = disk.filter(|_| view.dependencies().is_some());
        let key = disk.map(|_| disk_key(&inputs));
        let stored = disk
            .zip(key.as_ref())
            .and_then(|(disk, key)| load_entry(disk, key));
        let pack = if let Some((pack, files)) = stored {
            if let Some(dependencies) = view.dependencies() {
                dependencies.extend(files);
            }
            self.reuse_pixels(pack)
        } else {
            let (pack, blob) = compile(
                view,
                inputs.vanilla.as_deref(),
                inputs.vanilla_pack_dir.as_deref(),
                inputs.equipment.as_deref(),
                disk.is_some(),
                cancelled,
            )?;
            let pack = self.reuse_pixels(pack);
            if let (Some(disk), Some(key), Some(files)) = (disk, key, view.dependencies()) {
                let (disk, files, stored) = (disk.clone(), files.snapshot(), pack.clone());
                (self.store)(Box::new(move || {
                    store_entry(&disk, &key, &files, stored.as_deref(), blob.as_deref());
                }));
            }
            pack
        };
        let mut cache = self.lock();
        cache.retain(|(cached, _, _)| *cached != inputs);
        cache.push_front((
            inputs,
            pack.as_ref().map(Arc::downgrade),
            view.dependencies().map(|files| files.snapshot()),
        ));
        cache.truncate(2);
        Some(pack)
    }

    /// Shares unchanged rasters before another owner or disk writer retains the new catalog.
    fn reuse_pixels(&self, next: Option<Arc<SessionEntityPack>>) -> Option<Arc<SessionEntityPack>> {
        let previous = self
            .lock()
            .front()
            .and_then(|(_, pack, _)| pack.as_ref()?.upgrade());
        next.map(|next| {
            let next = match previous {
                Some(previous) => pixel_reuse::reuse(&previous, next),
                None => next,
            };
            next.with_shared_equipment_pixels()
        })
    }

    /// Recovers the bounded cache after a panicking reader.
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::VecDeque<CachedEntities>> {
        self.last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Names the stack, reference definitions, loaded equipment texture paths, and the build.
fn disk_key(inputs: &EntityInputs) -> [u8; 32] {
    use crate::compile_cache::{cache_key, part};
    use sha2::Digest;
    let mut key = cache_key("entities");
    for (id, version, subpack, content) in &inputs.stack {
        for bytes in [
            id.as_bytes(),
            version.as_bytes(),
            subpack.as_bytes(),
            content,
        ] {
            part(&mut key, bytes);
        }
    }
    part(&mut key, &vanilla_digest(inputs.vanilla.as_ref()));
    part(&mut key, &[u8::from(inputs.equipment.is_some())]);
    part(
        &mut key,
        &inputs
            .equipment
            .as_ref()
            .map(|catalog| catalog.texture_paths_sha256())
            .unwrap_or_default(),
    );
    if let Some(path) = &inputs.vanilla_pack_dir {
        part(&mut key, path.as_os_str().as_encoded_bytes());
    }
    key.finalize().into()
}

/// Digest of the refs' canonical JSON, computed once per installed refs.
fn vanilla_digest(refs: Option<&Arc<assets::VanillaEntityRefs>>) -> [u8; 32] {
    use sha2::Digest;
    type Memo = Option<(Arc<assets::VanillaEntityRefs>, [u8; 32])>;
    static MEMO: std::sync::Mutex<Memo> = std::sync::Mutex::new(None);
    let Some(refs) = refs else {
        return [0; 32];
    };
    let mut memo = MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((memoized, digest)) = memo.as_ref()
        && Arc::ptr_eq(memoized, refs)
    {
        return *digest;
    }
    let digest = sha2::Sha256::digest(refs.to_json()).into();
    *memo = Some((Arc::clone(refs), digest));
    digest
}

/// Streams the reads, then a presence flag and the pack, into `disk` under `key` without
/// staging an encoded copy of the pack's pixels. A pack without its entity blob or with an
/// unencodable catalog is not stored.
fn store_entry(
    disk: &crate::compile_cache::CompileCache,
    key: &[u8; 32],
    files: &std::collections::BTreeSet<resource_pack::PackDependency>,
    pack: Option<&SessionEntityPack>,
    blob: Option<&[u8]>,
) {
    let mut reads = Vec::new();
    resource_pack::PackDependency::encode_set(files, &mut reads);
    let encoding = match (pack, blob) {
        (None, _) => None,
        (Some(_), None) => return,
        (Some(pack), Some(blob)) => match pack.encoding(blob) {
            Ok(encoding) => Some(encoding),
            Err(error) => {
                tracing::debug!(%error, "compiled entity pack is not cacheable");
                return;
            }
        },
    };
    let length = READS_LENGTH as u64
        + reads.len() as u64
        + 1
        + encoding
            .as_ref()
            .map_or(0, |encoding| encoding.encoded_len());
    disk.store_with(key, length, |entry| {
        entry.write_all(&(reads.len() as u64).to_le_bytes())?;
        entry.write_all(&reads)?;
        entry.write_all(&[u8::from(encoding.is_some())])?;
        if let Some(encoding) = &encoding {
            encoding.write_to(entry)?;
        }
        Ok(())
    });
}

/// Bytes of the length prefix before an entry's encoded reads.
const READS_LENGTH: usize = std::mem::size_of::<u64>();

/// A stored pack and its reads, decoded straight from the cache entry; `None` for a miss
/// or an entry that does not decode.
fn load_entry(
    disk: &crate::compile_cache::CompileCache,
    key: &[u8; 32],
) -> Option<(
    Option<Arc<SessionEntityPack>>,
    std::collections::BTreeSet<resource_pack::PackDependency>,
)> {
    let decoded = disk.load_with(key, |entry, length| Some(decode_entry(entry, length)))?;
    if decoded.is_none() {
        tracing::warn!("discarding an undecodable compiled entity pack");
    }
    decoded
}

/// Reads what [`store_entry`] wrote: `length` bytes of reads, presence flag and pack.
fn decode_entry(
    entry: &mut dyn Read,
    length: u64,
) -> Option<(
    Option<Arc<SessionEntityPack>>,
    std::collections::BTreeSet<resource_pack::PackDependency>,
)> {
    let mut prefix = [0; READS_LENGTH];
    entry.read_exact(&mut prefix).ok()?;
    let reads_length = u64::from_le_bytes(prefix);
    let rest = reads_length
        .checked_add(READS_LENGTH as u64 + 1)
        .and_then(|used| length.checked_sub(used))?;
    let mut reads = Vec::new();
    (&mut *entry)
        .take(reads_length)
        .read_to_end(&mut reads)
        .ok()
        .filter(|read| *read as u64 == reads_length)?;
    let (files, []) = resource_pack::PackDependency::decode_set(&reads)? else {
        return None;
    };
    let mut present = [0];
    entry.read_exact(&mut present).ok()?;
    let pack = match (present[0], rest) {
        (0, 0) => None,
        (1, rest) => Some(Arc::new(SessionEntityPack::decode_from(entry, rest).ok()?)),
        _ => return None,
    };
    Some((pack, files))
}

pub(super) static ENTITY_CACHE: EntityCache = EntityCache {
    last: std::sync::Mutex::new(std::collections::VecDeque::new()),
    store: store_in_background,
};

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "reload_tests.rs"]
mod reload_tests;
