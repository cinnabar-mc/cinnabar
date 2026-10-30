//! One swap, one previous copy, and rollback when the newly launched client cannot start.

use crate::{platform, recovery, stage, target};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};
use time::OffsetDateTime;
use update_manifest::{Artifact, TrustedKeys, select::Check};

const PREPARED: &str = ".prepared-";
const UNPACKING: &str = ".unpacking-";

/// Local, signed inputs passed by the offline helper after the client chooses to update.
pub struct Request<'a> {
    pub stage_dir: &'a Path,
    pub target: &'a Path,
    pub keys: &'a TrustedKeys,
    pub check: Check<'a>,
    /// An explicit channel switch may replace only the build the user agreed to leave.
    pub switch_from: Option<&'a update_manifest::build::Build>,
    pub now: OffsetDateTime,
}

/// Verifies and applies an independently retained stage while owning the shared writer lease.
/// The caller must first observe the original client's exit.
pub fn after_exit(
    request: Request<'_>,
    lease: crate::lease::Lease,
    start: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let target_for_recovery = request.target;
    let mut touched = false;
    let mut started = false;
    let result = (|| {
        let Request {
            stage_dir,
            target,
            keys,
            check,
            switch_from,
            now,
        } = request;
        let root = stage_dir.parent().context("missing update directory")?;
        ensure!(
            root == lease.root(),
            "installation is outside the leased update directory"
        );
        // Authenticate even an obsolete queued stage before deciding to skip it.
        let selected = stage::verify_envelope(
            stage_dir,
            keys,
            Check {
                switch_now: true,
                ..check
            },
            now,
        )?;
        // A candidate unpacked before shutdown already passed the payload checks.
        let prepared = Some(prepared(target, &selected.artifact)?)
            .filter(|path| PREPARES && is_candidate(path));
        if prepared.is_none() {
            stage::verify_file(&stage_dir.join(stage::ARTIFACT_FILE), &selected.artifact)?;
        }
        let order = update_manifest::select::compare_builds(
            &selected.manifest.version,
            selected.manifest.build_number,
            check.current,
            check.current_build,
        )?;
        let consent_matches = switch_from.is_some_and(|build| {
            build.version == check.current && build.number == check.current_build
        });
        if order.is_eq() || (order.is_lt() && !consent_matches) {
            start(target)?;
            started = true;
            return Ok(());
        }
        ensure!(
            target::access(target)?.permits_update(),
            "install is not writable; use Download update"
        );
        ensure!(
            !fs::symlink_metadata(target)?.file_type().is_symlink(),
            "unsafe install target"
        );
        let os = check
            .platform
            .split_once('-')
            .context("invalid platform key")?
            .0;
        ensure!(
            os == std::env::consts::OS,
            "helper cannot apply another platform"
        );
        let parent = if os == "windows" {
            root
        } else {
            target.parent().context("missing install parent")?
        };
        let work = tempfile::Builder::new()
            .prefix(".cinnabar-update-")
            .tempdir_in(parent)?;
        if os == "windows" {
            let artifact = copy_payload(stage_dir, &selected.artifact, work.path())?;
            touched = true;
            platform::install_windows(&artifact, work.path(), target, &selected.artifact)?;
            remember_installer(root, &artifact)?;
            start(target)?;
            started = true;
        } else {
            let backup = target::backup(target)?;
            let candidate = match prepared {
                Some(candidate) => candidate,
                None => platform::candidate(
                    &copy_payload(stage_dir, &selected.artifact, work.path())?,
                    work.path(),
                    target,
                )?,
            };
            swap(target, &candidate, &backup)?;
            touched = true;
            if let Err(error) = start(target) {
                rollback(target, &backup, &work.path().join("failed"))?;
                return Err(error).context("new version did not start; previous version restored");
            }
            started = true;
            recovery::confirm_start(&backup)?;
            recovery::cleanup_confirmed_backup(target, &backup)?;
            // Candidates for other releases are only disk space now.
            let _ = prune_prepared(target, None);
        }
        // The handoff's reader lease and retention scan own snapshot cleanup.
        Ok(())
    })();
    match result {
        Err(error) if !touched => Err(error.context(Untouched)),
        Err(error) if !started => match recovery::restore_with_lease(target_for_recovery, &lease) {
            Ok(_) => Err(error),
            Err(recovery) => {
                Err(error.context(format!("recover changed installation: {recovery:#}")))
            }
        },
        result => result,
    }
}

/// Whether this platform swaps in a candidate unpacked beside the installation.
pub const PREPARES: bool = cfg!(any(target_os = "macos", target_os = "linux"));

/// Unpacks a staged release beside `target`, so the helper only swaps it in after exit.
/// Whoever can alter the unpacked copy could already alter the installation it replaces.
pub fn prepare(
    stage_dir: &Path,
    target: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
    lease: &crate::lease::Lease,
) -> Result<()> {
    ensure!(PREPARES, "this platform uses its installer");
    ensure!(
        stage_dir.parent() == Some(lease.root()),
        "stage is outside the leased update directory"
    );
    let selected = stage::verify_envelope(
        stage_dir,
        keys,
        Check {
            switch_now: true,
            ..check
        },
        now,
    )?;
    let prepared = prepared(target, &selected.artifact)?;
    prune_prepared(target, Some(&prepared))?;
    if is_candidate(&prepared) {
        return Ok(());
    }
    ensure!(
        target::access(target)?.permits_update(),
        "install is not writable"
    );
    let work = tempfile::Builder::new()
        .prefix(&sibling(target, UNPACKING)?)
        .tempdir_in(target.parent().context("missing install parent")?)?;
    let payload = copy_payload(stage_dir, &selected.artifact, work.path())?;
    let candidate = platform::candidate(&payload, work.path(), target)?;
    fs::rename(candidate, &prepared).context("publish prepared update")?;
    Ok(())
}

/// The complete candidate unpacked from the payload with this signed digest.
fn prepared(target: &Path, artifact: &Artifact) -> Result<PathBuf> {
    let mut name = sibling(target, PREPARED)?;
    name.push(artifact.sha256.to_ascii_lowercase());
    Ok(target.with_file_name(name))
}

/// Whether an unpacked candidate, not a link to one, sits at `path`.
fn is_candidate(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_symlink())
}

/// A hidden name prefix scoped to this installation's file name.
fn sibling(target: &Path, kind: &str) -> Result<OsString> {
    let mut name = OsString::from(".");
    name.push(target.file_name().context("missing install name")?);
    name.push(kind);
    Ok(name)
}

/// Removes this installation's other candidates and interrupted preparations.
/// Callers hold the update lease, so no other preparation is in progress.
fn prune_prepared(target: &Path, keep: Option<&Path>) -> Result<()> {
    let prefixes = [sibling(target, PREPARED)?, sibling(target, UNPACKING)?];
    for entry in fs::read_dir(target.parent().context("missing install parent")?)? {
        let entry = entry?;
        let name = entry.file_name();
        if keep != Some(entry.path().as_path())
            && prefixes.iter().any(|prefix| {
                name.as_encoded_bytes()
                    .starts_with(prefix.as_encoded_bytes())
            })
        {
            platform::release_interrupted(&entry.path());
            let _ = recovery::remove(&entry.path());
        }
    }
    Ok(())
}

/// Copies the staged payload into `work` and checks the copy against its signed digest.
fn copy_payload(stage_dir: &Path, artifact: &Artifact, work: &Path) -> Result<PathBuf> {
    let payload = work.join("payload");
    fs::copy(stage_dir.join(stage::ARTIFACT_FILE), &payload)?;
    stage::verify_file(&payload, artifact)?;
    Ok(payload)
}

/// A failed attempt made no changes to the installation and needs no rollback lease.
#[derive(Debug)]
pub struct Untouched;

impl std::fmt::Display for Untouched {
    /// Identifies pre-install failures without replacing their underlying error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("installation was not changed")
    }
}

/// Keeps the most recently downloaded previous MSI for manual repair, without copying Program Files.
fn remember_installer(root: &Path, artifact: &Path) -> Result<()> {
    let current = root.join("installed.msi");
    let previous = root.join("previous.msi");
    if current.exists() {
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(&current, &previous)?;
    }
    fs::copy(artifact, current)?;
    Ok(())
}

/// Promotes a file or complete app bundle, restoring it if the promotion itself fails.
pub(crate) fn swap(target: &Path, candidate: &Path, backup: &Path) -> Result<()> {
    recovery::cleanup_confirmed_backup(target, backup)?;
    ensure!(!backup.exists(), "previous installation is still retained");
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        recovery::prepare_exchange(target, backup)?;
        if let Err(error) = fs::rename(candidate, backup) {
            recovery::clear_preparation(backup)?;
            return Err(error).context("prepare replacement beside installation");
        }
        if let Err(error) = recovery::exchange(target, backup) {
            fs::rename(backup, candidate).context("retain uninstalled replacement")?;
            recovery::clear_preparation(backup)?;
            return Err(error).context("install update");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        fs::rename(target, backup).context("retain previous installation")?;
        if let Err(error) = fs::rename(candidate, target) {
            fs::rename(backup, target).context("restore previous installation")?;
            return Err(error).context("install update");
        }
    }
    Ok(())
}

/// Moves a failed new install aside before restoring the previous signed payload.
pub(crate) fn rollback(target: &Path, backup: &Path, failed: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        recovery::exchange(target, backup).context("restore previous installation")?;
        fs::rename(backup, failed).context("move failed new installation aside")?;
        recovery::clear_preparation(backup)?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        fs::rename(target, failed).context("move failed new installation aside")?;
        fs::rename(backup, target).context("restore previous installation")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
