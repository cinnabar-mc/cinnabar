//! Storage inventory reads the installed data roots; deletion never follows links.

use std::{fs, io, path::Path};

use launcher::install_layout::InstallLayout;

use launcher::menu::settings_storage::*;

#[path = "settings_storage_worker.rs"]
mod worker;
pub use worker::StorageReader;

/// Measures real world and downloaded-pack data once when Storage opens.
pub fn read_storage(layout: &InstallLayout) -> StorageView {
    let mut view = StorageView {
        opens_folders: crate::desktop::opens_folders(),
        accepts_file_drops: crate::desktop::accepts_file_drops(),
        ..StorageView::default()
    };
    match entries(&layout.local_worlds_dir()) {
        Ok(items) => view.worlds = items,
        Err(error) => view.error = Some(error.to_string()),
    }
    match cached_packs(&layout.resource_pack_cache_dir()) {
        Ok(items) => {
            for (index, category) in CATEGORIES.iter().enumerate() {
                view.expanded[index] = *category == "cache" && !items.is_empty();
            }
            view.cached = items;
        }
        Err(error) => view.error = Some(error.to_string()),
    }
    match entries(&layout.screenshots_dir()) {
        Ok(items) => {
            view.screenshots = items
                .into_iter()
                .filter(|item| {
                    item.path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
                        && fs::symlink_metadata(&item.path).is_ok_and(|metadata| metadata.is_file())
                })
                .collect()
        }
        Err(error) => view.error = Some(error.to_string()),
    }
    view
}

/// Inventories pack archives only; cache locks and partially written downloads stay private.
pub fn cached_packs(root: &Path) -> io::Result<Vec<StorageItem>> {
    let mut items = entries(root)?;
    items.retain(|item| {
        item.path
            .extension()
            .is_some_and(|extension| extension == "mcpack")
            && fs::symlink_metadata(&item.path).is_ok_and(|metadata| metadata.is_file())
    });
    for item in &mut items {
        if let Some(metadata) = fs::File::open(&item.path)
            .ok()
            .and_then(resource_pack::cached_pack_metadata)
        {
            item.icon = metadata.icon;
            item.name = metadata.pack.name;
            item.version = metadata.pack.version.map(|part| part.to_string()).join(".");
            item.pack_id = metadata.pack.id.to_string();
        }
    }
    Ok(items)
}

/// Counts ordinary entries without traversing links outside the requested directory.
pub fn entries(root: &Path) -> io::Result<Vec<StorageItem>> {
    let reader = match fs::read_dir(root) {
        Ok(reader) => reader,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut items = Vec::new();
    for entry in reader {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        items.push(StorageItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            bytes: tree_bytes(&entry.path())?,
            path: entry.path(),
            ..Default::default()
        });
    }
    items.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(items)
}

/// Measures file contents recursively while excluding symlink targets.
fn tree_bytes(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(0);
    }
    if !metadata.is_dir() {
        return Ok(metadata.len());
    }
    fs::read_dir(path)?.try_fold(0_u64, |total, entry| {
        Ok(total.saturating_add(tree_bytes(&entry?.path())?))
    })
}

/// Removes one inventoried cache entry, refusing linked roots and path escapes.
pub fn remove_entry(root: &Path, path: &Path) -> io::Result<()> {
    if path.parent() != Some(root)
        || root
            .ancestors()
            .any(|parent| fs::symlink_metadata(parent).is_ok_and(|metadata| metadata.is_symlink()))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache path is linked or outside its data directory",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cache entry is a symbolic link",
        ));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(test)]
#[path = "settings_storage_tests.rs"]
mod tests;
