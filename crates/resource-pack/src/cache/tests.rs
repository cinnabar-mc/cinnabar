use super::*;

/// Builds an original synthetic pack whose identity and bytes can change independently.
fn archive(id: Uuid, text: &str) -> Vec<u8> {
    let mut bytes = crate::test_support::archive_bytes(
        id,
        &[("texts/example.lang".into(), text.as_bytes().into())],
    );
    let length = bytes.len();
    let comment = b"original ZIP comment";
    bytes[length - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    bytes.extend_from_slice(comment);
    bytes
}

#[test]
fn persistent_cache_verifies_original_bytes_recovers_corruption_and_bounds_storage() {
    let root = tempfile::tempdir().unwrap();
    let cancelled = AtomicBool::new(false);
    let first = archive(Uuid::from_u128(1), "first");
    let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", first.len() as u64).unwrap();
    let cache = ArchiveCache::open(root.path(), first.len() as u64).unwrap();
    cache.store(&key, &first, &cancelled).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(first.clone())
    );
    assert!(ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).is_err());
    drop(cache);
    let cache = ArchiveCache::open(root.path(), first.len() as u64).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(first.clone())
    );
    let object = cache.objects().unwrap().pop().unwrap();
    let mut corrupt = first.clone();
    corrupt[0] ^= 1;
    std::fs::write(root.path().join("native-v1").join(object.name), corrupt).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        None
    );
    cache.store(&key, &first, &cancelled).unwrap();
    let second = archive(Uuid::from_u128(2), "other");
    let other = ArchiveCacheKey::new(Uuid::from_u128(2), "1.0.0", second.len() as u64).unwrap();
    cache.store(&other, &second, &cancelled).unwrap();
    assert!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cache
            .load(&other, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(second)
    );
    cancelled.store(true, Ordering::Relaxed);
    assert_eq!(
        cache.store(&key, &first, &cancelled).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(
        std::fs::read_dir(root.path().join("native-v1"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".partial-"))
    );
}

#[cfg(unix)]
#[test]
fn cache_refuses_symlinked_namespace_and_object() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("native-v1")).unwrap();
    assert!(ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).is_err());
    std::fs::remove_file(root.path().join("native-v1")).unwrap();
    let cache = ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).unwrap();
    let bytes = archive(Uuid::from_u128(1), "protected");
    let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", bytes.len() as u64).unwrap();
    let target = outside.path().join("source");
    std::fs::write(&target, &bytes).unwrap();
    let name = format!(
        "{}-{:x}{OBJECT_SUFFIX}",
        key.digest(),
        Sha256::digest(&bytes)
    );
    symlink(&target, root.path().join("native-v1").join(name)).unwrap();
    assert!(
        cache
            .load(&key, &AtomicBool::new(false), SystemTime::UNIX_EPOCH)
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read(target).unwrap(), bytes);
}

/// A rejected collision must survive quota admission and leave no staging objects behind.
#[test]
fn cache_store_preserves_colliding_regular_files_and_directories() {
    for directory in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let bytes = archive(Uuid::from_u128(1), "new archive");
        let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", bytes.len() as u64).unwrap();
        let cache = ArchiveCache::open(root.path(), bytes.len() as u64).unwrap();
        let namespace = root.path().join("native-v1");
        let destination = namespace.join(format!(
            "{}-{:x}{OBJECT_SUFFIX}",
            key.digest(),
            Sha256::digest(&bytes)
        ));
        let retained = if directory {
            std::fs::create_dir(&destination).unwrap();
            destination.join("retained")
        } else {
            destination
        };
        std::fs::write(&retained, b"existing destination").unwrap();
        assert_eq!(
            cache
                .store(&key, &bytes, &AtomicBool::new(false))
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(retained).unwrap(), b"existing destination");
        assert!(std::fs::read_dir(namespace).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".partial-")
        }));
    }
}

/// Storing an archive cannot replace a symlink or modify its target.
#[cfg(unix)]
#[test]
fn cache_store_preserves_symlink_collisions() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let bytes = archive(Uuid::from_u128(1), "new archive");
    let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", bytes.len() as u64).unwrap();
    let cache = ArchiveCache::open(root.path(), bytes.len() as u64).unwrap();
    let namespace = root.path().join("native-v1");
    let destination = namespace.join(format!(
        "{}-{:x}{OBJECT_SUFFIX}",
        key.digest(),
        Sha256::digest(&bytes)
    ));
    let target = outside.path().join("retained");
    std::fs::write(&target, b"existing target").unwrap();
    std::os::unix::fs::symlink(&target, &destination).unwrap();
    assert_eq!(
        cache
            .store(&key, &bytes, &AtomicBool::new(false))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(
        std::fs::symlink_metadata(&destination)
            .unwrap()
            .is_symlink()
    );
    assert_eq!(std::fs::read(target).unwrap(), b"existing target");
    assert!(std::fs::read_dir(namespace).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".partial-")
    }));
}
