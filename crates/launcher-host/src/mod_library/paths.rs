//! Component path normalization and stable missing-source identities.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Matches resolved entries, also resolving a missing companion through its existing parent.
pub fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || resolved_path(left)
            .zip(resolved_path(right))
            .is_some_and(|(left, right)| left == right)
}

/// Resolves directory aliases without requiring a not-yet-created settings entry to exist.
fn resolved_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
}

/// Resolves only directory aliases; a selected leaf link must still fail source admission.
pub fn resolved_component(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .zip(path.file_name())
        .map(|(parent, name)| parent.join(name))
        .unwrap_or_else(|| path.into())
}

/// Uses a resolved component path for panel ownership and cards without a source revision.
pub fn component_id(component: &Path) -> u64 {
    let digest = Sha256::digest(component.to_string_lossy().as_bytes());
    u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"))
}
