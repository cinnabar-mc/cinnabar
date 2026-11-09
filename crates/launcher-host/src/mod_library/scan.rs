//! Cached declarations and the current runtime status for each library card.

use super::{
    Backend, Selection,
    files::{file_identity, installed},
    paths::{component_id, same_path},
};
use launcher::mods::{ActiveMod, InstalledMod, Library};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Includes enabled external sources with file revisions, and missing sources by path alone.
pub(super) fn include_active(
    entries: &mut Vec<(InstalledMod, PathBuf)>,
    current: &[impl Selection],
) {
    for current in current.iter().filter(|current| current.enabled()) {
        let path = current.component();
        if entries
            .iter()
            .any(|(_, installed)| same_path(installed, path))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(path)
            .ok()
            .filter(fs::Metadata::is_file);
        entries.push((
            InstalledMod {
                id: metadata.as_ref().map_or_else(
                    || component_id(path),
                    |metadata| file_identity(path, metadata),
                ),
                file: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                bytes: metadata.as_ref().map_or(0, fs::Metadata::len),
                permissions: Vec::new(),
                metadata: Default::default(),
            },
            path.to_path_buf(),
        ));
    }
}

/// File identities invalidate cached declarations so unchanged large mods are not reread on polls.
pub fn scan_cached<B: Backend>(
    mods: &Path,
    registration: &Path,
    message: String,
    metadata: &mut BTreeMap<u64, B::Declarations>,
) -> Library {
    let mut installed = installed(mods);
    let current = B::registered(registration);
    include_active(&mut installed, &current);
    metadata.retain(|id, _| installed.iter().any(|(item, _)| item.id == *id));
    let active = current
        .iter()
        .filter(|current| current.enabled())
        .filter_map(|current| {
            installed
                .iter()
                .find(|(_, path)| same_path(path, current.component()))
                .map(|(item, _)| ActiveMod {
                    id: item.id,
                    status: current.status(),
                })
        })
        .collect();
    for (item, path) in &mut installed {
        let selected = current
            .iter()
            .find(|current| same_path(path, current.component()));
        let declared = metadata
            .entry(item.id)
            .or_insert_with(|| B::declarations(path));
        B::describe(item, declared, selected);
    }
    Library {
        update_progress: Vec::new(),
        installed: installed.into_iter().map(|(item, _)| item).collect(),
        active,
        busy: false,
        message,
        updates: Vec::new(),
    }
}
