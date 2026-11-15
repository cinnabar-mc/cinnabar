//! Unpacked resource packs in a development folder, as Bedrock's `development_resource_packs`.
//!
//! Each folder directly below the root that holds a resource manifest is listed beside imported
//! packs and read from disk on every preview instead of being copied into storage, so edits apply
//! on the next Apply without importing again or changing the manifest version. A development pack
//! shadows an imported pack with the same UUID.

use super::{GlobalPackLibrary, InstalledPack, LibraryError, read_bounded};
use crate::{AdmissionError, MAX_MANIFEST_BYTES, parser::MANIFEST_NAMES};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// One folder below the development root that holds a resource manifest.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DevelopmentPack {
    pub(super) metadata: InstalledPack,
    pub(super) path: PathBuf,
}

/// The development root and what its last scan found.
#[derive(Default)]
pub(super) struct Development {
    root: Option<PathBuf>,
    pub(super) packs: Vec<DevelopmentPack>,
    rejected: Vec<String>,
}

impl GlobalPackLibrary {
    /// Lists unpacked packs below `root`, creating the folder so players can find it. Selected
    /// development packs whose folder is gone leave the selection and are named in
    /// [`Self::missing_packs`].
    pub fn set_development_root(&mut self, root: impl Into<PathBuf>) {
        let root = root.into();
        // A missing folder only lists no packs; the next scan reports nothing either.
        let _ = fs::create_dir_all(&root);
        self.development.root = Some(root);
        let before: Vec<Uuid> = self.active.iter().map(|active| active.id).collect();
        self.scan_development();
        self.missing.extend(
            before
                .into_iter()
                .filter(|id| !self.active.iter().any(|active| active.id == *id))
                .map(|id| format!("development pack {id}")),
        );
    }

    /// Reads the development folder again and reports whether its packs or problems changed.
    /// Packs whose folder is gone leave the staged selection, as Bedrock ignores a configured
    /// pack it cannot find.
    pub fn scan_development(&mut self) -> bool {
        let Some(root) = &self.development.root else {
            return false;
        };
        let (packs, rejected) = scan(root);
        let changed = packs != self.development.packs || rejected != self.development.rejected;
        self.development.packs = packs;
        self.development.rejected = rejected;
        let staged = std::mem::take(&mut self.active);
        let kept: Vec<_> = staged
            .into_iter()
            .filter(|active| self.metadata(active).is_some())
            .collect();
        self.active = kept;
        changed
    }

    /// Folders the last scan could not list, each as `folder: reason`.
    pub fn development_rejections(&self) -> &[String] {
        &self.development.rejected
    }

    /// Whether `id` names a listed development pack, which is read from its folder.
    pub fn is_development(&self, id: Uuid) -> bool {
        self.development_pack(id).is_some()
    }

    /// The listed development pack with `id`.
    pub(super) fn development_pack(&self, id: Uuid) -> Option<&DevelopmentPack> {
        self.development
            .packs
            .iter()
            .find(|pack| pack.metadata.id == id)
    }
}

impl DevelopmentPack {
    /// Archives the folder as it is now for ordinary validation, naming the folder in failures.
    pub(super) fn archive(&self) -> Result<Vec<u8>, LibraryError> {
        crate::import::directory::archive(&self.path).map_err(|error| self.error(error))
    }

    /// Reads the folder's own icon; a pack without one has none.
    pub(super) fn icon(&self) -> Result<Option<Box<[u8]>>, LibraryError> {
        match read_bounded(&self.path.join("pack_icon.png"), crate::MAX_PACK_ICON_BYTES) {
            Ok(bytes) => Ok(Some(bytes.into_boxed_slice())),
            Err(LibraryError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Adds the folder name to a failure without changing its reason.
    pub(super) fn error(&self, source: LibraryError) -> LibraryError {
        LibraryError::PackFile {
            path: folder_name(&self.path),
            source: Box::new(source),
        }
    }
}

/// Lists resource packs in folders directly below `root` in folder-name order; the first folder
/// keeps a UUID that a later one repeats. Behavior packs are skipped silently.
fn scan(root: &Path) -> (Vec<DevelopmentPack>, Vec<String>) {
    let mut packs: Vec<DevelopmentPack> = Vec::new();
    let mut rejected = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return (packs, rejected);
    };
    // A linked folder counts, so a pack can stay in its project and be linked here.
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()))
        .collect();
    folders.sort();
    for path in folders {
        let name = folder_name(&path);
        match read_pack(&path) {
            Ok(Some(metadata)) => {
                if packs.iter().any(|pack| pack.metadata.id == metadata.id) {
                    rejected.push(format!(
                        "{name}: another development pack already uses UUID {}",
                        metadata.id
                    ));
                } else {
                    packs.push(DevelopmentPack { metadata, path });
                }
            }
            Ok(None) => {}
            Err(error) => rejected.push(format!("{name}: {error}")),
        }
    }
    (packs, rejected)
}

/// Reads one folder's list metadata with the same manifest rules as an import.
fn read_pack(path: &Path) -> Result<Option<InstalledPack>, LibraryError> {
    let manifest = MANIFEST_NAMES
        .iter()
        .map(|name| path.join(name))
        .find(|path| path.is_file())
        .ok_or(AdmissionError::MissingManifest)?;
    let bytes = read_bounded(&manifest, MAX_MANIFEST_BYTES)?;
    let Some(mut metadata) = crate::import::read_metadata(&bytes)? else {
        return Ok(None);
    };
    if let Some(bytes) = read_optional(&path.join("contents.json"))? {
        crate::import::reject_encrypted_index(&bytes)?;
    }
    // Display names are optional, so an unreadable language file keeps the manifest's text.
    if let Ok(Some(language)) = read_optional(&path.join("texts/en_US.lang")) {
        crate::import::localize_metadata(&mut metadata, &language);
    }
    Ok(Some(metadata))
}

/// Reads a small optional pack file; an absent file is `None`.
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, LibraryError> {
    match read_bounded(path, MAX_MANIFEST_BYTES) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(LibraryError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The folder's own name for messages.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests;
