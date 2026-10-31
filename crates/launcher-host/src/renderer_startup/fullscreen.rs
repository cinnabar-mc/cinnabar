//! Journals fullscreen entry separately from graphics-backend startup failures.

use super::{owner::Owner, save, termination};
use crate::video_settings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const SENTINEL: &str = "fullscreen-transition.json";
const LOCK: &str = "fullscreen-transition.lock";

#[derive(Serialize, Deserialize)]
struct Pending {
    owner: Owner,
}

/// Holds an exclusive display-mode journal through entry and its completed presentation.
#[derive(Debug)]
pub struct FullscreenGuard {
    root: PathBuf,
    _lease: Arc<termination::Lease>,
    owner: Owner,
    pub recovered: bool,
}

impl FullscreenGuard {
    /// Recovers an interrupted entry before settings are applied, preserving other video values.
    pub fn open(root: &Path) -> Result<Self> {
        let mut active = termination::active()?;
        fs::create_dir_all(root)?;
        let file = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(LOCK))?;
        file.try_lock()
            .map_err(|error| match error {
                fs::TryLockError::WouldBlock => std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "another client owns fullscreen recovery",
                ),
                fs::TryLockError::Error(error) => error,
            })
            .context("another client owns fullscreen recovery")?;
        let owner = Owner::current()?;
        let recovered = super::load::<Pending>(&root.join(SENTINEL))?.is_some();
        if recovered {
            let mut settings = video_settings::load(root)?;
            settings.fullscreen = false;
            video_settings::save(root, settings)?;
            fs::remove_file(root.join(SENTINEL))?;
        }
        let lease = Arc::new(termination::Lease(file));
        active.register(&root.join(SENTINEL), &lease);
        Ok(Self {
            root: root.to_owned(),
            _lease: lease,
            owner,
            recovered,
        })
    }

    /// Flushes attribution before the native window receives a fullscreen request.
    pub fn begin(&mut self) -> Result<()> {
        let _active = termination::active()?;
        save(
            &self.root.join(SENTINEL),
            &Pending {
                owner: self.owner.clone(),
            },
        )
    }

    /// Clears attribution after presentation or an orderly exit, without changing preferences.
    pub fn clear(&mut self) -> Result<()> {
        let _active = termination::active()?;
        match fs::remove_file(self.root.join(SENTINEL)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
