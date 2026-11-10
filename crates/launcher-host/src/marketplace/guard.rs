//! Small shared authority snapshot; frame checks perform no filesystem or network I/O.

use super::verify::{CLOCK_SKEW_MS, Manifest, RevocationState, verify_bytes};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

#[derive(Default)]
struct State {
    epoch: u64,
    account: String,
    issued_at: u64,
    expires_at: u64,
    revoked: HashMap<String, String>,
    installed: HashMap<PathBuf, (String, Manifest)>,
}

impl State {
    /// Checks the live account, signed freshness window and withdrawals for one release.
    fn authorizes(&self, owner: &str, hash: &str, now: u64) -> bool {
        self.account == owner
            && self.expires_at > now
            && self.issued_at <= now.saturating_add(CLOCK_SKEW_MS)
            && !self.revoked.contains_key(hash)
    }

    /// Returns an installed receipt's manifest only while its release is currently authorized.
    fn admitted(&self, path: &Path, now: u64) -> Option<&Manifest> {
        self.installed
            .get(path)
            .filter(|(owner, manifest)| self.authorizes(owner, &manifest.sha256, now))
            .map(|(_, manifest)| manifest)
    }

    /// Produces plain text for the existing bounded Mods status renderer.
    fn reason(&self, path: &Path, now: u64) -> String {
        if let Some((owner, manifest)) = self.installed.get(path) {
            if let Some(reason) = self.revoked.get(&manifest.sha256) {
                return launcher::mods::marketplace_withdrawn(reason);
            }
            if owner != &self.account {
                return "Sign into this mod's account before enabling it.".into();
            }
            if self.expires_at <= now {
                return "Marketplace checks expired. Reconnect and reload the mod after checks refresh.".into();
            }
        }
        "Marketplace release held: current signed evidence is unavailable.".into()
    }
}

/// Live admission for managed files, independent of browser trust or persisted grants.
#[derive(Default)]
pub struct Guard(Mutex<State>);

impl Guard {
    /// Locks the snapshot, recovering it after a panicking holder.
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Switches identity immediately; existing receipts cannot grant another account authority.
    pub fn configure(&self, epoch: u64) {
        let mut state = self.state();
        state.epoch = epoch;
        state.account.clear();
    }

    /// Applies an authenticated identity only if its control request still belongs to this epoch.
    pub fn account(&self, epoch: u64, xuid: String) {
        let mut state = self.state();
        if state.epoch == epoch {
            state.account = xuid;
        }
    }

    /// Rechecks an uninstalled download against live account and withdrawal authority.
    pub fn permits(&self, owner: &str, hash: &str, now: u64) -> bool {
        !owner.is_empty() && self.state().authorizes(owner, hash, now)
    }

    /// Retains every known revoked hash while advancing the signed freshness checkpoint.
    pub fn revocations(&self, value: RevocationState) {
        let mut state = self.state();
        state.issued_at = value.issued_at;
        state.expires_at = value.expires_at;
        state.revoked.extend(value.revoked);
    }

    /// Returns the last accepted issue time so a replay cannot roll back this session.
    pub fn checkpoint(&self) -> u64 {
        self.state().issued_at
    }

    /// Registers a receipt only after its signature, declaration and on-disk bytes are verified.
    pub fn installed(&self, path: PathBuf, owner: String, manifest: Manifest) {
        self.state().installed.insert(path, (owner, manifest));
    }

    /// Lists the managed paths registered with verified receipts this session, present or not.
    pub fn installed_paths(&self) -> Vec<PathBuf> {
        self.state().installed.keys().cloned().collect()
    }

    /// Forgets a retired source so it cannot regain authority from an in-memory receipt.
    pub(super) fn forget(&self, path: &Path) {
        self.state().installed.remove(path);
    }

    /// Lists known older releases for an authorized successor of the same project and account.
    pub fn predecessors(&self, path: &Path, now: u64) -> Vec<PathBuf> {
        let state = self.state();
        let Some((owner, next)) = state.installed.get(path) else {
            return Vec::new();
        };
        if !state.authorizes(owner, &next.sha256, now) {
            return Vec::new();
        }
        state
            .installed
            .iter()
            .filter(|(old_path, (old_owner, old))| {
                old_path.as_path() != path
                    && owner == old_owner
                    && next.slug == old.slug
                    && update_manifest::select::newer(&next.version, &old.version).unwrap_or(false)
            })
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Tests freshness and ownership at installation and immediately before frame admission.
    pub fn allows(&self, path: &Path, now: u64) -> bool {
        !managed(path) || self.state().admitted(path, now).is_some()
    }

    /// Returns the signed manifest of a managed file while its release is currently authorized.
    ///
    /// Reads no source bytes, so callers can bound a registration's authority before capturing
    /// the source; [`Self::check`] still verifies those bytes before admission.
    pub fn manifest(&self, path: &Path, now: u64) -> Option<Manifest> {
        self.state().admitted(path, now).cloned()
    }

    /// Authorizes one installed release for `owner` until `expires_at`, as verified current
    /// revocation evidence would, so admission tests need no signer.
    #[cfg(any(test, feature = "test-support"))]
    pub fn authorized(path: PathBuf, owner: &str, manifest: Manifest, expires_at: u64) -> Self {
        let guard = Self::default();
        guard.configure(1);
        guard.account(1, owner.into());
        guard.revocations(RevocationState {
            issued_at: 0,
            expires_at,
            revoked: HashMap::new(),
        });
        guard.installed(path, owner.into(), manifest);
        guard
    }

    /// Validates a captured source against its receipt without reading mutable files again.
    ///
    /// Returns the signed manifest of a managed file for declaration checks, or `None` for a
    /// local file.
    pub fn check(&self, path: &Path, bytes: &[u8], now: u64) -> Result<Option<Manifest>> {
        if !managed(path) {
            return Ok(None);
        }
        let manifest = {
            let state = self.state();
            state
                .admitted(path, now)
                .cloned()
                .ok_or_else(|| anyhow::Error::msg(state.reason(path, now)))?
        };
        verify_bytes(bytes, &manifest)?;
        ensure!(
            self.allows(path, now),
            "Marketplace authority changed while source bytes were checked."
        );
        Ok(Some(manifest))
    }

    /// Blocks a withdrawal hint immediately, pending the next signed revocation refresh.
    pub fn hold(&self, hash: &str) {
        self.state().revoked.entry(hash.into()).or_default();
    }

    /// Explains a held file using signed withdrawal text or the current account/freshness state.
    pub fn reason(&self, path: &Path, now: u64) -> String {
        self.state().reason(path, now)
    }
}

/// Reserves managed filenames even if a receipt was deleted or corrupted.
pub fn managed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(launcher::mods::is_marketplace_file)
}
