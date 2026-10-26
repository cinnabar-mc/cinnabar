use super::*;
use crate::test_support::{Dir, pack_archive, write_vanilla_manifest};
use sha2::{Digest, Sha256};

/// Stores a verified fixture archive at a version-specific manifest path.
fn version(root: &Path, name: &str, texture: &[u8]) -> PackPaths {
    let archive = pack_archive(texture);
    write_vanilla_manifest(
        root,
        "https://example.invalid/pack.zip",
        &format!("{:x}", Sha256::digest(&archive)),
        &format!("{name}.zip"),
    );
    let manifest = Sources::Kit(root.into()).resolve(VANILLA_MANIFEST);
    let mut source: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    source["cache_dir"] = format!(".local/assets/bedrock-samples/{name}/full").into();
    fs::write(manifest, source.to_string()).unwrap();
    let paths = pack_paths(root, root).unwrap().1;
    fs::create_dir_all(paths.archive.parent().unwrap()).unwrap();
    fs::write(&paths.archive, archive).unwrap();
    paths
}

#[test]
fn extracted_resource_versions_survive_archive_pruning_and_switching_back() {
    let dir = Dir::new("pack-versions");
    let cancel = AtomicBool::new(false);
    let first = version(dir.path(), "first", b"first");
    assert!(!pack_cached(dir.path(), dir.path()).unwrap());
    unpack(dir.path(), dir.path(), &cancel).unwrap();
    assert!(pack_cached(dir.path(), dir.path()).unwrap());

    let second = version(dir.path(), "second", b"second");
    assert!(!pack_cached(dir.path(), dir.path()).unwrap());
    unpack(dir.path(), dir.path(), &cancel).unwrap();
    prune(dir.path(), dir.path());
    assert!(!first.archive.exists());
    assert!(second.archive.exists());
    assert!(first.cache.is_dir());

    version(dir.path(), "first", b"first");
    fs::remove_file(&first.archive).unwrap();
    assert!(pack_cached(dir.path(), dir.path()).unwrap());
    unpack(dir.path(), dir.path(), &cancel).unwrap();
    assert_eq!(
        fs::read(first.cache.join("resource_pack/textures/block.png")).unwrap(),
        b"first"
    );
}

#[test]
fn changed_archive_bytes_at_the_same_version_replace_the_extracted_cache() {
    let dir = Dir::new("pack-repin");
    let cancel = AtomicBool::new(false);
    version(dir.path(), "same", b"old");
    unpack(dir.path(), dir.path(), &cancel).unwrap();
    let changed = version(dir.path(), "same", b"new");
    assert!(!pack_cached(dir.path(), dir.path()).unwrap());
    unpack(dir.path(), dir.path(), &cancel).unwrap();
    assert!(pack_cached(dir.path(), dir.path()).unwrap());
    assert_eq!(
        fs::read(changed.cache.join("resource_pack/textures/block.png")).unwrap(),
        b"new"
    );
    for missing in [PACK_STAMP, "resource_pack/blocks.json"] {
        fs::remove_file(changed.cache.join(missing)).unwrap();
        assert!(!pack_cached(dir.path(), dir.path()).unwrap());
        unpack(dir.path(), dir.path(), &cancel).unwrap();
        assert!(pack_cached(dir.path(), dir.path()).unwrap());
    }
}
