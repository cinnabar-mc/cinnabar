//! Disk inventory and deletion boundary regressions.

use super::*;
use std::path::PathBuf;

/// Gives each test a private, canonical data directory.
fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-storage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    fs::canonicalize(path).unwrap()
}

#[test]
fn inventory_measures_nested_contents_and_cache_clear_preserves_other_data() {
    let root = fixture();
    let cache = root.join("objects");
    let pack = cache.join("pack");
    fs::create_dir_all(&pack).unwrap();
    fs::write(pack.join("manifest"), [0; 13]).unwrap();
    fs::write(pack.join("texture"), [0; 29]).unwrap();
    fs::write(root.join("account"), b"keep").unwrap();
    let items = entries(&cache).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].bytes, 42);
    remove_entry(&cache, &items[0].path).unwrap();
    assert!(entries(&cache).unwrap().is_empty());
    assert_eq!(fs::read(root.join("account")).unwrap(), b"keep");
    assert!(remove_entry(&cache, &root.join("account")).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn linked_roots_and_entries_are_never_deleted_or_counted() {
    let root = fixture();
    let cache = root.join("objects");
    fs::create_dir_all(&cache).unwrap();
    let outside = root.join("keep");
    fs::write(&outside, b"private").unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("escape")).unwrap();
    assert!(entries(&cache).unwrap().is_empty());
    assert!(remove_entry(&cache, &cache.join("escape")).is_err());
    fs::write(cache.join("pack"), [0; 5]).unwrap();
    let linked = root.join("linked");
    std::os::unix::fs::symlink(&cache, &linked).unwrap();
    assert!(remove_entry(&linked, &linked.join("pack")).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"private");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn downloaded_pack_inventory_excludes_housekeeping_and_reads_display_identity() {
    use std::io::Write;
    let root = fixture();
    let path = root.join("opaque.mcpack");
    let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    archive
        .start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(br#"{"format_version":2,"header":{"name":"pack.name","uuid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]},"modules":[{"type":"resources","uuid":"bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]}]}"#).unwrap();
    archive
        .start_file("texts/en_US.lang", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"pack.name=Cloud Islands\n").unwrap();
    archive
        .start_file("pack_icon.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"encoded icon").unwrap();
    archive.finish().unwrap();
    fs::write(root.join(".packcache.lock"), b"lease").unwrap();
    fs::write(root.join(".packcache-partial"), b"partial").unwrap();
    fs::write(root.join("broken.mcpack"), b"broken").unwrap();
    let items = cached_packs(&root).unwrap();
    assert_eq!(items.len(), 2);
    let pack = items.iter().find(|item| item.path == path).unwrap();
    assert_eq!(pack.name, "Cloud Islands");
    assert_eq!(pack.version, "1.2.3");
    assert_eq!(pack.icon.as_deref(), Some(b"encoded icon".as_slice()));
    assert_eq!(pack.pack_id, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
    assert!(pack.matches_query("CLOUD"));
    assert!(pack.matches_query("1.2.3"));
    assert!(pack.matches_query("aaaaaaaa"));
    assert!(pack.matches_query("opaque"));
    assert!(!pack.matches_query("another pack"));
    assert!(items.iter().any(|item| item.name == "broken.mcpack"));
    fs::remove_dir_all(root).unwrap();
}
