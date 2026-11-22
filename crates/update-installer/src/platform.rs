//! Native package preparation starts only after signed artifact verification.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use anyhow::{Result, bail};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Prepares a complete bundle, extracted AppDir or executable AppImage on the install filesystem.
pub(crate) fn candidate(artifact: &Path, work: &Path, target: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos::candidate(artifact, work, target)
    }
    #[cfg(target_os = "linux")]
    {
        linux::candidate(artifact, work, target)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (artifact, work, target);
        bail!("this platform uses its installer")
    }
}

/// Releases what an unpacking cut short by exit can leave behind in `work`.
pub(crate) fn release_interrupted(work: &Path) {
    #[cfg(target_os = "macos")]
    macos::release_interrupted(work);
    #[cfg(not(target_os = "macos"))]
    let _ = work;
}

/// The raw msiexec command line for a passive upgrade into the existing `target`, which may be a
/// custom `INSTALLFOLDER`. msiexec parses its own command line, so values are quoted as
/// `NAME="value"`; Windows paths never contain quotes.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn msiexec_arguments(msi: &Path, target: &Path) -> Vec<OsString> {
    vec![
        "/i".into(),
        quoted("", msi),
        "/passive".into(),
        "/norestart".into(),
        "REBOOT=ReallySuppress".into(),
        // The helper owns the single client restart after installation completes.
        "MSIDISABLERMRESTART=1".into(),
        quoted("INSTALLFOLDER=", target),
    ]
}

/// `prefix"path"`, where no backslash may escape the closing quote: trailing separators are
/// dropped, and a bare drive or root keeps its backslash doubled, which parses as one.
#[cfg_attr(not(windows), allow(dead_code))]
fn quoted(prefix: &str, path: &Path) -> OsString {
    let bytes = path.as_os_str().as_encoded_bytes();
    let end = bytes
        .iter()
        .rposition(|&byte| !matches!(byte, b'\\' | b'/'))
        .map_or(0, |last| last + 1);
    // SAFETY: only trailing ASCII separators are cut, which leaves valid encoded bytes.
    let value = unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(&bytes[..end]) };
    let mut argument = OsString::from(format!("{prefix}\""));
    argument.push(value);
    if end == 0 || bytes[end - 1] == b':' {
        argument.push(r"\\");
    }
    argument.push("\"");
    argument
}

/// Helper mode that removes the per-machine install a Windows update moved from; runs elevated.
pub const REMOVE_MACHINE_FLAG: &str = "--remove-machine-install";

/// Where a Windows update of `target` installs. A per-user package cannot write the per-machine
/// install it replaces, so that one moves to the recorded or default per-user folder; any other
/// install updates in place.
pub(crate) fn windows_destination(target: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        use anyhow::Context;
        let install = update_manifest::platforms::windows_install();
        let local = std::env::var_os("LOCALAPPDATA").context("missing LOCALAPPDATA")?;
        let default = Path::new(&local)
            .join(&install.parent)
            .join(&install.folder);
        let machine = !windows::machine_products()?.is_empty();
        Ok(destination(
            target,
            windows::recorded_user_folder().as_deref(),
            default,
            machine,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        bail!("Windows installer cannot run on this platform")
    }
}

/// `target` is the per-user install when it is the recorded folder, and otherwise the per-machine
/// one whenever a per-machine product is installed.
#[cfg_attr(not(windows), allow(dead_code))]
fn destination(
    target: &Path,
    recorded: Option<&Path>,
    default: PathBuf,
    machine_installed: bool,
) -> PathBuf {
    let same = |folder: &Path| {
        let key = |path: &Path| {
            path.components()
                .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
        };
        key(folder) == key(target)
    };
    if !machine_installed || recorded.is_some_and(same) {
        target.to_owned()
    } else {
        recorded.map_or(default, Path::to_owned)
    }
}

/// Asks for administrator approval to remove the per-machine install an update just moved from.
/// The update already succeeded, so a declined or failed removal only leaves that copy behind.
#[cfg_attr(
    windows,
    expect(
        clippy::print_stderr,
        reason = "the launcher captures the helper's stderr into apply.log"
    )
)]
pub(crate) fn remove_migrated_machine_install() {
    #[cfg(windows)]
    if let Err(error) = windows::run_elevated(REMOVE_MACHINE_FLAG) {
        eprintln!("Kept the previous machine-wide install: {error:#}");
    }
}

/// Removes per-machine bundles and MSIs from before per-user packages; requires administrator rights.
pub fn remove_machine_install() -> Result<()> {
    #[cfg(windows)]
    {
        windows::remove_machine_install()
    }
    #[cfg(not(windows))]
    {
        bail!("Windows installer cannot run on this platform")
    }
}

/// Runs Windows Installer passively with system-managed permission consent and no forced reboot.
pub(crate) fn install_windows(
    artifact: &Path,
    work: &Path,
    target: &Path,
    expected: &update_manifest::Artifact,
) -> Result<()> {
    #[cfg(windows)]
    {
        windows::install(artifact, work, target, expected)
    }
    #[cfg(not(windows))]
    {
        let _ = (artifact, work, target, expected);
        bail!("Windows installer cannot run on this platform")
    }
}

#[cfg(test)]
mod tests;
