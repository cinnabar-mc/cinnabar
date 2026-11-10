//! Exact first-party assets with verified digests, bounded staging and cancellation fencing.

use super::{Candidate, progress::Progress};
use anyhow::{Context, Result, ensure};
use launcher::mods::UpdatePhase;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Deserialize)]
pub(super) struct Asset {
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) digest: Option<String>,
    #[serde(skip)]
    pub(super) url: Option<String>,
    #[serde(skip)]
    pub(super) revision_update: Option<ui::mod_metadata::RevisionModUpdate>,
}

/// A verified release artifact. Dropping it removes its temporary file.
pub struct Downloaded {
    pub revision_update: Option<Box<ui::mod_metadata::RevisionModUpdate>>,
    pub id: u64,
    pub version: String,
    pub repository: String,
    pub asset: String,
    file: tempfile::NamedTempFile,
    generation: Arc<AtomicU64>,
    revision: u64,
}

impl Downloaded {
    /// The complete, SHA-256-verified component, awaiting host validation.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Whether player preferences still authorize this download; the installer rechecks its file.
    pub fn is_current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.revision
    }

    /// Creates an isolated artifact for installer tests, without starting a network worker.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(id: u64, version: &str, repository: &str, asset: &str, bytes: &[u8]) -> Self {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        Self {
            revision_update: ui::mod_metadata::RevisionModUpdate::parse(serde_json::json!({
                "source": format!("https://github.com/{repository}/releases/latest/download/{asset}"),
                "package": ui::mod_metadata::OFFICIAL_UPDATE_PACKAGE,
                "revision": format!("{:x}", Sha256::digest(version.trim_start_matches('v').as_bytes()))[..40],
                "api_revision": "a".repeat(40),
            }).to_string().as_bytes()).map(Box::new),
            id,
            version: version.into(),
            repository: repository.into(),
            asset: asset.into(),
            file,
            generation: Arc::new(AtomicU64::new(1)),
            revision: 1,
        }
    }

    /// Invalidates a fixture to exercise cancellation after download.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cancel_fixture(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// Selects exactly one named, bounded WASM asset with a usable SHA-256 digest.
pub(super) fn select<'a>(assets: &'a [Asset], name: &str, limit: usize) -> Option<&'a Asset> {
    let mut matching = assets.iter().filter(|asset| asset.name == name);
    let asset = matching.next()?;
    if matching.next().is_some()
        || asset.size == 0
        || asset.size > limit as u64
        || !name.ends_with(".wasm")
        || digest(asset).is_none()
    {
        return None;
    }
    Some(asset)
}

/// Decodes only GitHub's SHA-256 digest format, rejecting missing or malformed hashes.
fn digest(asset: &Asset) -> Option<[u8; 32]> {
    let hex = asset.digest.as_ref()?.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (byte, [high, low]) in bytes.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
        let high = char::from(*high).to_digit(16)?;
        let low = char::from(*low).to_digit(16)?;
        *byte = ((high << 4) | low) as u8;
    }
    Some(bytes)
}

/// Builds the asset client separately from the API client, which rejects all redirects.
pub(super) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(launcher::PRODUCT_USER_AGENT)
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Library revisions cancel obsolete transfers; preference revisions also fence installation.
pub(super) struct Fence {
    pub generation: Arc<AtomicU64>,
    pub revision: u64,
    pub permission: Arc<AtomicU64>,
    pub authorization: u64,
    pub progress: Progress,
}

/// Fetches the compatible service's exact immutable URL and verifies its digest.
pub(super) async fn fetch(
    client: &reqwest::Client,
    candidate: &Candidate,
    version: &str,
    asset: &Asset,
    fence: Fence,
    limit: usize,
    staging: &Path,
) -> Result<Downloaded> {
    fence.progress.set(
        candidate.id,
        fence.revision,
        UpdatePhase::Downloading,
        0,
        asset.size,
    );
    let response = client
        .get(
            asset
                .url
                .as_ref()
                .context("missing official download URL")?,
        )
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await?
        .error_for_status()?;
    receive(response, candidate, version, asset, fence, limit, staging).await
}

/// Stages one response without trusting its length, framing, or contents.
async fn receive(
    mut response: reqwest::Response,
    candidate: &Candidate,
    version: &str,
    asset: &Asset,
    fence: Fence,
    limit: usize,
    staging: &Path,
) -> Result<Downloaded> {
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size == asset.size),
        "asset length changed"
    );
    std::fs::create_dir_all(staging).context("create mod download staging directory")?;
    let mut file = tempfile::NamedTempFile::new_in(staging).context("stage mod download")?;
    let mut length = 0;
    let mut hash = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            fence.generation.load(Ordering::SeqCst) == fence.revision,
            "mod download cancelled"
        );
        length += chunk.len();
        ensure!(
            length <= limit && length as u64 <= asset.size,
            "mod download exceeds byte limit"
        );
        hash.update(&chunk);
        file.write_all(&chunk)?;
        fence.progress.set(
            candidate.id,
            fence.revision,
            UpdatePhase::Downloading,
            length as u64,
            asset.size,
        );
    }
    ensure!(length as u64 == asset.size, "incomplete mod download");
    fence.progress.set(
        candidate.id,
        fence.revision,
        UpdatePhase::Verifying,
        length as u64,
        asset.size,
    );
    let expected = digest(asset).context("missing mod SHA-256")?;
    ensure!(
        hash.finalize().as_slice() == expected,
        "mod download SHA-256 mismatch"
    );
    Ok(Downloaded {
        revision_update: asset.revision_update.clone().map(Box::new),
        id: candidate.id,
        version: version.into(),
        repository: candidate.repository.clone(),
        asset: asset.name.clone(),
        file,
        generation: fence.permission,
        revision: fence.authorization,
    })
}

#[cfg(test)]
mod tests;
