//! Scanning, complete-file publication and fenced deletion.

use super::{Backend, Retirement};
use launcher::mods::{COMPONENT_EXTENSION, InstalledMod};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Validates a chosen component and copies it into the mods directory; returns its file name.
pub fn import<B: Backend>(mods: &Path, source: &Path) -> Result<String, String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|_| is_component(source))
        .ok_or_else(|| format!("Choose a .{COMPONENT_EXTENSION} mod component."))?;
    if name.starts_with('.') {
        return Err("Mod file names must not start with a dot.".into());
    }
    let bytes =
        B::read_source(source).map_err(|error| format!("Could not read {name}: {error}"))?;
    B::validate_source(&bytes).map_err(|error| format!("{name} is not a valid mod: {error:#}"))?;
    publish_component(mods, name, &bytes)?;
    Ok(name.to_owned())
}

/// Publishes complete bytes under a new name without overwriting any existing file or link.
pub(super) fn publish_component(mods: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(mods).map_err(|error| format!("Could not install {name}: {error}"))?;
    let staging = mods.join(format!(".{name}.{}.importing", std::process::id()));
    let mut file = fs::File::create_new(&staging)
        .map_err(|error| format!("Could not stage {name}: {error}"))?;
    let result = file.write_all(bytes).and_then(|()| {
        // Android forbids app hard links; tempfile uses a no-overwrite rename there.
        tempfile::TempPath::try_from_path(&staging)?
            .persist_noclobber(mods.join(name))
            .map_err(|error| error.error)
    });
    drop(file);
    let _ = fs::remove_file(&staging);
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!("A mod named {name} is already installed. Choose a different file name.")
        } else {
            format!("Could not install {name}: {error}")
        }
    })
}

/// Rechecks the removal's selection before deleting each retired component or owned companion.
pub(super) fn remove<B: Backend>(
    path: &Path,
    companion: Option<&Path>,
    registration: &Path,
    receipt: &B::Receipt,
    permit: &B::Permit,
) -> Result<(), String> {
    for file in std::iter::once(path).chain(companion) {
        B::confirm(receipt, registration)?;
        permit.confirm()?;
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && file != path => {}
            Err(error) => return Err(format!("Could not remove the mod: {error}")),
        }
    }
    Ok(())
}

/// Recognizes the component extension accepted by the Mods tab.
fn is_component(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(COMPONENT_EXTENSION))
}

/// Lists visible components by file name; staging files have a different extension.
pub fn installed(mods: &Path) -> Vec<(InstalledMod, PathBuf)> {
    let Ok(entries) = fs::read_dir(mods) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file = path.file_name()?.to_str()?.to_owned();
            let metadata = fs::symlink_metadata(&path)
                .ok()
                .filter(fs::Metadata::is_file)?;
            (!file.starts_with('.') && is_component(&path)).then(|| {
                let item = InstalledMod {
                    id: file_identity(&path, &metadata),
                    file,
                    bytes: metadata.len(),
                    permissions: Vec::new(),
                    metadata: Default::default(),
                };
                (item, path)
            })
        })
        .collect();
    found.sort_by(|(left, _), (right, _)| left.file.cmp(&right.file));
    found
}

/// Identifies a component's path and file revision without rereading its contents.
pub(super) fn file_identity(path: &Path, metadata: &fs::Metadata) -> u64 {
    let mut digest = Sha256::new();
    digest.update(path.as_os_str().as_encoded_bytes());
    digest.update(metadata.len().to_le_bytes());
    for timestamp in [metadata.created(), metadata.modified()]
        .into_iter()
        .flatten()
    {
        if let Ok(elapsed) = timestamp.duration_since(std::time::UNIX_EPOCH) {
            digest.update(elapsed.as_nanos().to_le_bytes());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(metadata.dev().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
    }
    let digest = digest.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"))
}
