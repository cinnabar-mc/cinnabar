//! Replacement of an opted-in installed component, without changing registration or grants.

use super::{Backend, Selection, files::installed, paths::same_path};
use crate::mod_updates::Downloaded;
use crossbeam_channel::Sender;
use launcher::mods::{Library, Status};
use std::time::{Duration, Instant};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Validates the exact download and atomically replaces its unchanged installed source.
/// Existing registration, permission grants and settings companions retain their identities.
pub fn install<B: Backend>(mods: &Path, download: &Downloaded) -> Result<Installed, String> {
    let (item, path) = selected(mods, download)?;
    let old =
        B::read_source(&path).map_err(|error| format!("Could not read {}: {error}", item.file))?;
    let bytes = B::read_source(download.path())
        .map_err(|error| format!("Could not read mod update: {error}"))?;
    validate::<B>(&old, &bytes, download)
        .map_err(|error| format!("Could not update {}: {error}", item.file))?;
    let permissions = fs::symlink_metadata(&path)
        .map_err(|error| error.to_string())?
        .permissions();
    if permissions.readonly() {
        return Err(format!("{} is read-only.", item.file));
    }
    let mut staging = tempfile::NamedTempFile::new_in(mods).map_err(|error| error.to_string())?;
    staging
        .write_all(&bytes)
        .and_then(|()| staging.as_file().sync_all())
        .and_then(|()| staging.as_file().set_permissions(permissions))
        .map_err(|error| format!("Could not stage {}: {error}", item.file))?;
    selected(mods, download)?;
    // Re-read immediately before replacement: an external editor may preserve file metadata.
    let current = B::read_source(&path).map_err(|error| error.to_string())?;
    if current != old {
        return Err("The installed mod changed while its update was prepared.".into());
    }
    if !download.is_current() {
        return Err("Automatic mod update cancelled.".into());
    }
    staging
        .persist(&path)
        .map_err(|error| format!("Could not replace {}: {error}", item.file))?;
    // The existing source watcher reloads changed component bytes and retains the running
    // instance if activation fails; no registration write can grant additional permissions.
    Ok(Installed {
        path,
        message: format!("Updated {} to {}.", item.file, download.version),
    })
}

/// The installed file and its completion message, before any enabled instance reloads.
pub struct Installed {
    path: PathBuf,
    pub message: String,
}

/// Completion is held until the runtime acknowledges the freshly requested instance.
pub struct PendingReload {
    pub id: u64,
    deadline: Instant,
    message: String,
    finished: Sender<()>,
}

impl PendingReload {
    /// Requests reload under a fresh registration request ID, preserving the existing grants.
    pub fn start<B: Backend>(
        mods: &Path,
        registration: &Path,
        installed: Installed,
        request: String,
        finished: Sender<()>,
        now: Instant,
    ) -> Result<Option<Self>, String> {
        if !B::registered(registration)
            .iter()
            .any(|entry| entry.enabled() && same_path(entry.component(), &installed.path))
        {
            let _ = finished.send(());
            return Ok(None);
        }
        let id = super::files::installed(mods)
            .into_iter()
            .find(|(_, path)| same_path(path, &installed.path))
            .map(|(item, _)| item.id)
            .ok_or("The updated mod file changed before reload.")?;
        B::apply(
            launcher::mods::Action::Reload(id),
            registration,
            Some(&installed.path),
            request,
        )
        .map_err(|error| {
            format!("The update was installed, but reload could not start: {error}")
        })?;
        Ok(Some(Self {
            id,
            deadline: now + Duration::from_secs(60),
            message: installed.message,
            finished,
        }))
    }

    /// Reports success only from the status belonging to the new registration request.
    pub fn poll(&self, library: &Library, now: Instant) -> Option<Result<String, String>> {
        if !library.installed.iter().any(|item| item.id == self.id) {
            return Some(Err(
                "The installed mod changed while its update was reloading.".into(),
            ));
        }
        match library.active.iter().find(|active| active.id == self.id).map(|active| &active.status) {
            Some(Status::Starting) if now < self.deadline => None,
            Some(Status::Starting) => Some(Err("The update was installed, but reload has not finished. Use Reload or restart Cinnabar.".into())),
            Some(Status::Failed(error) | Status::ReloadFailed(error)) => Some(Err(format!("The update was installed, but reload failed: {error}"))),
            Some(Status::Running | Status::NextJoin) | None => Some(Ok(self.message.clone())),
        }
    }

    /// Releases the frame-side busy state after an acknowledged completion or explicit failure.
    pub fn finish(&self) {
        let _ = self.finished.send(());
    }
}

/// Restricts automatic replacement to the same visible regular file in the managed mods folder.
fn selected(
    mods: &Path,
    download: &Downloaded,
) -> Result<(launcher::mods::InstalledMod, PathBuf), String> {
    if !download.is_current() {
        return Err("Automatic mod update cancelled.".into());
    }
    installed(mods)
        .into_iter()
        .find(|(item, _)| item.id == download.id)
        .ok_or_else(|| "The installed mod changed before its update was ready.".into())
}

/// Requires both snapshots to retain the exact opted-in source and the promised newer version.
fn validate<B: Backend>(old: &[u8], bytes: &[u8], download: &Downloaded) -> Result<(), String> {
    let previous = B::update_metadata(old, false)?;
    let next = B::update_metadata(bytes, true)?;
    let matches_source = |metadata: &ui::mod_metadata::ModMetadata| {
        metadata
            .revision_update
            .as_ref()
            .map(|r| &r.source)
            .or(metadata.update_url.as_ref())
            .is_some_and(|source| {
                source.repository().slug() == download.repository
                    && source.asset() == download.asset
            })
    };
    if !matches_source(&previous) || !matches_source(&next) {
        return Err("The update source does not match the installed mod.".into());
    }
    if let Some(expected) = download.revision_update.as_deref() {
        if next.revision_update.as_ref() != Some(expected)
            || !previous
                .revision_update
                .as_ref()
                .is_some_and(|old| old.package == expected.package && old.source == expected.source)
        {
            return Err(
                "The downloaded mod identity does not match the compatible release.".into(),
            );
        }
    } else if previous.revision_update.is_some() || next.revision_update.is_some() {
        return Err("A revision-aware update requires verified compatibility metadata.".into());
    }
    let current = previous
        .version
        .as_deref()
        .ok_or("The installed mod has no version.")?;
    let next_version = next
        .version
        .as_deref()
        .ok_or("The update has no version.")?;
    let normalize = |version: &str| version.strip_prefix('v').unwrap_or(version).to_owned();
    if normalize(next_version) != normalize(&download.version)
        || !update_manifest::select::newer(next_version, current)
            .map_err(|error| error.to_string())?
    {
        return Err("The downloaded version does not match a newer release.".into());
    }
    B::validate_source(bytes).map_err(|error| format!("invalid mod: {error:#}"))
}
