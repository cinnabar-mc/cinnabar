//! Retires immutable archives only after their replacement has been acknowledged.
use super::{
    ActivePack, CATALOG_FILE, Catalog, GlobalPackLibrary, InstalledPack, LibraryError,
    atomic_write, encode_catalog,
};
use std::{fs, path::Path};

/// Drops entries whose archive is gone, as Bedrock ignores a configured pack it cannot find, and
/// names the packs the player loses. A failed save only repeats this on the next open.
pub(super) fn forget_missing(root: &Path, catalog: &mut Catalog) -> Vec<String> {
    let gone = |pack: &InstalledPack| matches!(root.join(pack.filename()).try_exists(), Ok(false));
    let selects = |active: &ActivePack, pack: &InstalledPack| {
        active.id == pack.id && active.revision == pack.revision
    };
    let lost: Vec<InstalledPack> = catalog
        .available
        .iter()
        .chain(&catalog.retained)
        .filter(|pack| gone(pack))
        .cloned()
        .collect();
    if lost.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = lost
        .iter()
        .filter(|pack| {
            catalog.available.contains(pack)
                || catalog.active.iter().any(|active| selects(active, pack))
        })
        .map(|pack| pack.name.clone())
        .collect();
    names.sort();
    names.dedup();
    catalog.available.retain(|pack| !lost.contains(pack));
    catalog.retained.retain(|pack| !lost.contains(pack));
    let installed = |active: &ActivePack| !lost.iter().any(|pack| selects(active, pack));
    catalog.active.retain(installed);
    catalog.restore.retain(installed);
    if let Ok(bytes) = encode_catalog(catalog) {
        let _ = atomic_write(&root.join(CATALOG_FILE), &bytes);
    }
    names
}

/// Removes obsolete revisions after the newest preview has been acknowledged.
pub(super) fn prune(root: &Path, catalog: &mut Catalog) -> Result<(), LibraryError> {
    let mut candidate = catalog.clone();
    let mut failure = None;
    candidate.retained.retain(|pack| {
        if candidate
            .active
            .iter()
            .chain(&candidate.restore)
            .any(|active| active.id == pack.id && active.revision == pack.revision)
        {
            return true;
        }
        match fs::remove_file(root.join(pack.filename())) {
            Ok(()) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                failure.get_or_insert(error);
                true
            }
        }
    });
    atomic_write(&root.join(CATALOG_FILE), &encode_catalog(&candidate)?)?;
    *catalog = candidate;
    match failure {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

impl GlobalPackLibrary {
    /// Preserves a durable restore selection's archive revisions until a new backup replaces it.
    pub fn retain_for_restore(&mut self, selection: &[ActivePack]) -> Result<(), LibraryError> {
        if selection.len() > crate::MAX_PACKS {
            return Err(crate::AdmissionError::TooManyPacks.into());
        }
        if self.catalog.restore == selection {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.catalog.restore, selection.to_vec());
        if let Err(error) = self.persist() {
            self.catalog.restore = previous;
            return Err(error);
        }
        Ok(())
    }
}
