//! Local single-player worlds: the saved-world store, the official dedicated-server download and
//! the local server processes. The core only learns the open world's loopback address.

mod android_setup;
mod android_trust;
mod bds;
mod code_cache;
mod container;
mod core_link;
mod docker;
mod download;
mod import;
mod junction;
mod level_dat;
mod manager;
mod ports;
mod process;
mod provision;
mod startup_timing;
mod store;
#[cfg(test)]
mod test_support;
mod translated;
mod translated_setup;
mod unpack;
mod world;

pub use android_setup::{android_server_needs_preparation, prepare_android_server};
pub use android_trust::refresh as refresh_android_server_trust;
pub use core_link::{CoreLink, CoreRoute};
pub use import::{WORLD_ARCHIVE_EXTENSION, is_world_archive};
pub use manager::{Host, Manager};
pub use world::WorldError;

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use launcher::install_layout::InstallLayout;
use serde::Deserialize;

const BEDROCK_TARGET_JSON: &str = include_str!("../../../../assets/bedrock-target.json");

/// The dedicated-server build local worlds run, from the target manifest.
#[derive(Debug, Deserialize)]
struct ServerPin {
    server_version: String,
    bds_container_image: String,
}

fn server_pin() -> &'static ServerPin {
    static PIN: OnceLock<ServerPin> = OnceLock::new();
    PIN.get_or_init(|| {
        serde_json::from_str(BEDROCK_TARGET_JSON).expect("valid Bedrock target manifest")
    })
}

/// Opens the install's local worlds on the manifest's exact server build and image. Hosts without
/// a native server or bundled translator probe Docker in the background, assuming a container
/// runtime meanwhile.
pub fn open(layout: &InstallLayout) -> Result<Manager, WorldError> {
    let pin = server_pin();
    let store = Arc::new(store::Store::open(layout.local_worlds_dir())?);
    let platform = provision::Platform::host();
    let translation = translated::Translation::detect(
        platform,
        &layout.core_executable,
        android_runtime_dir(&layout.user_data_root),
    )
    .map(Arc::new);
    let (runtime, pending) = if let Some(translation) = &translation {
        (translation.runtime(), false)
    } else if platform.runs_bds() {
        (provision::RuntimeInfo::native(), false)
    } else if platform.runs_containers() {
        (provision::RuntimeInfo::checking(), true)
    } else {
        (docker::detect_runtime(platform, docker::DOCKER, &[]), false)
    };
    let provisioner = Arc::new(provision::Provisioner::new(
        layout.bds_dir(),
        pin.server_version.clone(),
    ));
    provisioner.set_runtime(runtime.clone());
    if let Some(translation) = &translation {
        android_setup::configure_preparation(&provisioner, translation);
    }
    let detected = translation
        .as_ref()
        .map(|translation| translation.runtime());
    provisioner.set_detector(Arc::new(move || {
        detected
            .clone()
            .unwrap_or_else(|| docker::detect_runtime(platform, docker::DOCKER, &[]))
    }));
    let mut parts = manager::Parts::new(store);
    let mut bds = bds::BdsRunner::new(Arc::clone(&provisioner), pin.bds_container_image.clone());
    bds.translation = translation;
    parts.runner = Some(Arc::new(bds));
    parts.setup = Some(Arc::clone(&provisioner));
    let manager = Manager::from_parts(parts);
    tracing::info!(
        "local worlds in {} ({}: {})",
        layout.local_worlds_dir().display(),
        runtime.kind.as_str(),
        runtime.reason
    );
    if pending {
        provisioner.detect_in_background(runtime);
    }
    Ok(manager)
}

/// The Android server's exported platform certificate bundle, refreshed before each app launch.
pub fn android_server_trust_path(user_data_root: &Path) -> PathBuf {
    translated::trust_path(&android_runtime_dir(user_data_root))
}

/// Keeps the translated server's cache and trust store beneath one private runtime directory.
fn android_runtime_dir(user_data_root: &Path) -> PathBuf {
    user_data_root.join("bds-runtime")
}

/// A manager over the worlds in `root` with no server runners, for code that drives one in tests.
#[cfg(any(test, feature = "test-support"))]
pub fn open_store(root: std::path::PathBuf) -> Result<Manager, WorldError> {
    let store = Arc::new(store::Store::open(root.clone())?);
    let mut parts = manager::Parts::new(store);
    let setup = Arc::new(provision::Provisioner::new(
        root.join(".bds-test"),
        String::new(),
    ));
    setup.set_runtime(provision::RuntimeInfo::native());
    setup.accept_eula()?;
    parts.setup = Some(setup);
    Ok(Manager::from_parts(parts))
}

/// Ends a start or another wait once its [`CancelHandle`] is dropped.
#[derive(Clone)]
pub(crate) struct Cancel(Receiver<()>);

/// Cancels its [`Cancel`] when dropped.
pub(crate) struct CancelHandle {
    _sender: Sender<()>,
}

pub(crate) fn cancel_pair() -> (CancelHandle, Cancel) {
    let (sender, receiver) = crossbeam_channel::bounded(0);
    (CancelHandle { _sender: sender }, Cancel(receiver))
}

impl Cancel {
    /// A cancellation that never fires.
    pub(crate) fn never() -> Self {
        Self(crossbeam_channel::never())
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(self.0.try_recv(), Err(TryRecvError::Disconnected))
    }

    /// Disconnects once cancelled, for `select!`.
    pub(crate) fn receiver(&self) -> &Receiver<()> {
        &self.0
    }
}

/// Locks `mutex`, recovering the data if a holder panicked.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Creates `path` and its parents, readable only by this user on Unix.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Writes `bytes` to `path`, creating it readable only by this user on Unix.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(path)?, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag alone drifts with upstream releases and can break joins; the image must name a digest.
    #[test]
    fn the_container_image_is_pinned_to_a_digest() {
        let image = &server_pin().bds_container_image;
        let (name, digest) = image.split_once("@sha256:").expect("digest-pinned image");
        assert!(!name.is_empty() && !name.ends_with(":latest"), "{image}");
        assert!(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "{image}"
        );
        let version = &server_pin().server_version;
        assert_eq!(version.split('.').count(), 4, "exact build: {version}");
    }

    #[test]
    fn dropping_the_handle_cancels_every_clone() {
        let (handle, cancel) = cancel_pair();
        let clone = cancel.clone();
        assert!(!cancel.is_cancelled());
        drop(handle);
        assert!(cancel.is_cancelled() && clone.is_cancelled());
        assert!(!Cancel::never().is_cancelled());
    }
}
