//! Serialized account delivery, bounded public HTTP and signed revocation refreshes.

use super::{
    Guard, Keys, SignedRelease, SignedRevocations, now_ms,
    storage::{self, Downloaded, Receipt},
};
use anyhow::{Result, ensure};
use bridge::contract::{CinnabarModEntry, CinnabarModLibrary};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

pub(super) const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_REVOCATIONS_BYTES: usize = 1024 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Sustained rate a slow link must reach for a signed asset to finish within its timeout.
const MIN_ASSET_BYTES_PER_SECOND: u64 = 128 * 1024;
const MAX_ASSET_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const REFRESH_INTERVAL_MS: u64 = 60_000;
const RETRY_BASE_MS: u64 = 30_000;
const RETRY_MAX_MS: u64 = 30 * 60_000;

/// Public transport is fixed by the client contract; only in-module tests substitute loopback HTTP.
struct Transport {
    client: reqwest::Client,
    origin: url::Url,
}

type Request = (String, String, String, String, String);

/// Queue admission deduplicates one account epoch; only verified receipts survive a switch.
#[derive(Default)]
struct Deliveries {
    epoch: Option<u64>,
    requests: HashSet<Request>,
    failures: HashMap<Request, Failure>,
}

/// Consecutive failed attempts of one send and the earliest time it may be fetched again.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Failure {
    count: u32,
    retry_at: u64,
}

impl Deliveries {
    /// Forgets cancelled queue-only work while retaining successful and locally removed sends.
    fn refresh(
        &mut self,
        epoch: u64,
        mods: &Path,
        keys: Option<&Keys>,
        limit: usize,
        guard: &Guard,
        now: u64,
    ) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.failures.clear();
        self.requests = keys
            .map(|keys| storage::restore(mods, keys, limit, guard, now))
            .unwrap_or_default()
            .iter()
            .map(|receipt| request(&receipt.owner, &receipt.selected))
            .collect();
        self.epoch = Some(epoch);
    }

    /// Records a send only when the compatibility worker has accepted its complete download.
    ///
    /// A full queue counts as a failed attempt, so the send is retried after its backoff.
    fn queue(
        &mut self,
        key: Request,
        prepared: Downloaded,
        outgoing: &Sender<Downloaded>,
        now: u64,
    ) {
        if outgoing.try_send(prepared).is_ok() {
            self.failures.remove(&key);
            self.requests.insert(key);
        } else {
            self.fail(key, now);
        }
    }

    /// Reports whether a send is unconsumed and past any failure backoff at `now`.
    fn due(&self, key: &Request, now: u64) -> bool {
        !self.requests.contains(key) && self.retry_due(key, now)
    }

    /// Reports whether a request has no failure backoff pending at `now`.
    fn retry_due(&self, key: &Request, now: u64) -> bool {
        self.failures
            .get(key)
            .is_none_or(|failure| now >= failure.retry_at)
    }

    /// Delays the next attempt of a failed send by its exponential backoff from `now`.
    fn fail(&mut self, key: Request, now: u64) {
        let failure = self.failures.entry(key).or_insert(Failure {
            count: 0,
            retry_at: 0,
        });
        failure.count = failure.count.saturating_add(1);
        failure.retry_at = now.saturating_add(retry_delay(failure.count));
    }
}

/// Returns the wait after `failures` consecutive failures: 30 s doubling up to 30 minutes.
fn retry_delay(failures: u32) -> u64 {
    RETRY_BASE_MS
        .saturating_mul(1 << failures.saturating_sub(1).min(16))
        .min(RETRY_MAX_MS)
}

/// Allows a signed asset the base request time plus its transfer at a conservative slow-link
/// rate, never exceeding the hard asset ceiling.
fn asset_timeout(bytes: u64) -> Duration {
    let transfer = Duration::from_millis(bytes.saturating_mul(1000) / MIN_ASSET_BYTES_PER_SECOND);
    REQUEST_TIMEOUT
        .saturating_add(transfer)
        .min(MAX_ASSET_TIMEOUT)
}

/// Identifies an explicit website send, preserving local removals until the player sends it again.
fn request(owner: &str, selected: &CinnabarModEntry) -> Request {
    (
        owner.into(),
        selected.slug.clone(),
        selected.version.clone(),
        selected.sha256.clone(),
        selected.sent_at.clone(),
    )
}

/// Performs one bounded public GET with no credentials, arbitrary URLs, redirects or partial
/// responses; `timeout` covers the whole exchange including the body.
async fn fetch(
    transport: &Transport,
    path: &str,
    limit: usize,
    timeout: Duration,
) -> Result<(Vec<u8>, Option<String>)> {
    let url = transport.origin.join(path)?;
    let mut response = transport.client.get(url).timeout(timeout).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "Marketplace returned {}.",
        response.status()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= limit as u64),
        "Marketplace response exceeds its byte limit."
    );
    let signature = response
        .headers()
        .get("X-Cinnabar-Signature")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "Marketplace response exceeds its byte limit."
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, signature))
}

/// Verifies and persists fresh revocations before allowing any new installation.
async fn refresh(
    transport: &Transport,
    mods: &Path,
    keys: &Keys,
    guard: &Guard,
    now: impl Fn() -> u64,
) -> Result<()> {
    let (bytes, signature) = fetch(
        transport,
        "/v1/mods/revocations",
        MAX_REVOCATIONS_BYTES,
        REQUEST_TIMEOUT,
    )
    .await?;
    let signed = SignedRevocations {
        body: String::from_utf8(bytes)?,
        signature: signature.ok_or_else(|| anyhow::anyhow!("Missing revocation signature."))?,
    };
    let state = signed.verify(keys, now(), guard.checkpoint())?;
    // Persist before advancing the in-memory checkpoint, so restarts retain rollback protection.
    storage::save(
        &storage::revocations_path(mods),
        &serde_json::to_vec(&signed)?,
    )?;
    guard.revocations(state);
    Ok(())
}

/// Retrieves the exact signed release, streams its digest-bound file and fences account changes.
#[allow(
    clippy::too_many_arguments,
    reason = "download binds public transport, source ceiling and account cancellation fence"
)]
async fn download(
    transport: &Transport,
    mods: &Path,
    keys: &Keys,
    owner: &str,
    selected: &CinnabarModEntry,
    limit: usize,
    fence: (Arc<AtomicU64>, u64, Arc<Guard>),
    now: impl Fn() -> u64,
) -> Result<Downloaded> {
    let (generation, epoch, guard) = fence;
    #[derive(Deserialize)]
    struct Response {
        release: SignedRelease,
    }
    // Encode the path segment even though the local bridge admits only canonical selectors.
    let mut url = transport.origin.clone();
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("Invalid marketplace origin."))?
        .extend([
            "v1",
            "mods",
            &selected.slug,
            "releases",
            &selected.version,
            &selected.sha256,
        ]);
    let (body, _) = fetch(transport, url.path(), MAX_RESPONSE_BYTES, REQUEST_TIMEOUT).await?;
    let release = serde_json::from_slice::<Response>(&body)?.release;
    let manifest = release.verify(keys, selected, limit, now())?;
    let (bytes, _) = fetch(
        transport,
        &format!("/v1/mods/assets/{}", manifest.sha256),
        manifest.bytes as usize,
        asset_timeout(manifest.bytes),
    )
    .await?;
    super::verify::verify_bytes(&bytes, &manifest)?;
    ensure!(
        generation.load(Ordering::SeqCst) == epoch && guard.permits(owner, &manifest.sha256, now()),
        "Marketplace download cancelled or held."
    );
    let mut file = tempfile::NamedTempFile::new_in(mods)?;
    file.write_all(&bytes)?;
    Ok(Downloaded {
        file,
        manifest,
        receipt: Receipt {
            owner: owner.into(),
            selected: selected.clone(),
            release,
        },
        generation,
        epoch,
        guard,
        mods: mods.into(),
    })
}

/// Bounds one local control call so a stalled core cannot block delivery.
fn control<T>(
    runtime: &tokio::runtime::Runtime,
    call: impl Future<Output = Result<T, bridge::BridgeError>>,
) -> Option<T> {
    runtime
        .block_on(async { tokio::time::timeout(CONTROL_TIMEOUT, call).await })
        .ok()?
        .ok()
}

/// Delivery state and authority shared by every pass of one worker.
struct Worker<'a> {
    runtime: &'a tokio::runtime::Runtime,
    transport: &'a Transport,
    mods: &'a Path,
    keys: Option<&'a Keys>,
    limit: usize,
    guard: Arc<Guard>,
    generation: Arc<AtomicU64>,
    outgoing: &'a Sender<Downloaded>,
    deliveries: Deliveries,
    refresh_at: u64,
}

impl Worker<'_> {
    /// Whether a verified managed file is still installed, so its freshness window must not lapse.
    fn protects_installed(&self) -> bool {
        self.guard
            .installed_paths()
            .iter()
            .any(|path| path.exists())
    }

    /// Whether `library` holds a send that is neither withdrawn nor already consumed.
    fn pending(&self, library: &CinnabarModLibrary) -> bool {
        library.mods.iter().any(|selected| {
            !selected.pulled
                && !self
                    .deliveries
                    .requests
                    .contains(&request(&library.xuid, selected))
        })
    }

    /// Verifies fresh revocations once the previous refresh interval has elapsed at `now`.
    ///
    /// Refreshes only while there is something to protect or deliver: an installed managed file
    /// or any selection in the current account `library`. Otherwise no request is made and the
    /// next one is due as soon as either appears. A failure is reported only when it affects an
    /// installed managed file or a pending selection; without fresh evidence those stay held.
    fn refresh_due(
        &mut self,
        library: Option<&CinnabarModLibrary>,
        now: &impl Fn() -> u64,
        notify: &impl Fn(String),
    ) {
        let Some(keys) = self.keys else {
            return;
        };
        if now() < self.refresh_at {
            return;
        }
        let installed = self.protects_installed();
        if !installed && library.is_none_or(|library| library.mods.is_empty()) {
            return;
        }
        if let Err(error) =
            self.runtime
                .block_on(refresh(self.transport, self.mods, keys, &self.guard, now))
            && (installed || library.is_some_and(|library| self.pending(library)))
        {
            notify(format!("Marketplace checks could not refresh: {error:#}"));
        }
        self.refresh_at = now().saturating_add(REFRESH_INTERVAL_MS);
    }

    /// Delivers each unfinished selection of one account library snapshot.
    ///
    /// `confirm` reconfirms the live account and exact selection after network I/O, and
    /// `remove` asks the core to drop a locally removed send. Held, consumed and backed-off
    /// sends are skipped without network I/O. Each removal request waits for the failure
    /// backoff before it is sent again; a completed removal leaves the library, which forgets it.
    fn pass(
        &mut self,
        epoch: u64,
        library: &CinnabarModLibrary,
        confirm: impl Fn(&CinnabarModEntry) -> bool,
        remove: impl Fn(&CinnabarModEntry),
        now: &impl Fn() -> u64,
        notify: &impl Fn(String),
    ) {
        for selected in library.mods.iter().filter(|selected| selected.pulled) {
            self.guard.hold(&selected.sha256);
        }
        let Some(keys) = self.keys else {
            if !library.mods.is_empty() {
                notify("Marketplace downloads are not enabled in this build. Update Cinnabar or import a local mod.".into());
            }
            return;
        };
        let owner = &library.xuid;
        self.deliveries.failures.retain(|key, _| {
            library
                .mods
                .iter()
                .any(|selected| request(owner, selected) == *key)
        });
        for selected in &library.mods {
            if self.generation.load(Ordering::SeqCst) != epoch {
                return;
            }
            let key = request(owner, selected);
            if storage::removed(self.mods, owner, selected, keys, self.limit, now()) {
                if self.deliveries.retry_due(&key, now()) {
                    remove(selected);
                    self.deliveries.fail(key, now());
                }
                continue;
            }
            if selected.pulled || !self.deliveries.due(&key, now()) {
                continue;
            }
            // A long pass must not outlive the signed freshness window it started with.
            self.refresh_due(Some(library), now, notify);
            if !self.guard.permits(owner, &selected.sha256, now()) {
                continue;
            }
            notify(format!("Preparing {} from the marketplace…", selected.slug));
            match self.runtime.block_on(download(
                self.transport,
                self.mods,
                keys,
                owner,
                selected,
                self.limit,
                (self.generation.clone(), epoch, self.guard.clone()),
                now,
            )) {
                Ok(prepared) if confirm(selected) => {
                    self.deliveries.queue(key, prepared, self.outgoing, now());
                }
                Ok(_) => self.deliveries.fail(key, now()),
                Err(error) => {
                    self.deliveries.fail(key, now());
                    notify(format!("Marketplace mod could not be prepared: {error:#}"));
                }
            }
        }
    }
}

/// Owns one off-frame delivery loop and exits when the app drops its request channel.
#[allow(
    clippy::too_many_arguments,
    reason = "one worker owns its bounded channels, identity fence and runtime authority"
)]
pub(super) fn run(
    mods: PathBuf,
    limit: usize,
    guard: Arc<Guard>,
    generation: Arc<AtomicU64>,
    context: Arc<Mutex<Option<PathBuf>>>,
    incoming: Receiver<()>,
    outgoing: Sender<Downloaded>,
    events: &Sender<String>,
) -> Result<()> {
    let keys = Keys::parse(include_bytes!("../../data/marketplace-keys.json")).ok();
    fs::create_dir_all(&mods)?;
    let mods = mods.canonicalize()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let transport = Transport {
        client,
        origin: bridge::contract::CINNABAR_API_BASE_URL.parse()?,
    };
    if let Some(keys) = &keys
        && let Ok(bytes) = storage::read(
            &storage::revocations_path(&mods),
            2 * MAX_REVOCATIONS_BYTES + 1024,
        )
        && let Ok(signed) = serde_json::from_slice::<SignedRevocations>(&bytes)
        && let Ok(state) = signed.restore(keys, now_ms())
    {
        guard.revocations(state);
    }
    let notify = |message: String| {
        let _ = events.try_send(message);
    };
    let mut worker = Worker {
        runtime: &runtime,
        transport: &transport,
        mods: &mods,
        keys: keys.as_ref(),
        limit,
        guard: guard.clone(),
        generation: generation.clone(),
        outgoing: &outgoing,
        deliveries: Deliveries::default(),
        refresh_at: 0,
    };
    loop {
        let (socket, epoch) = {
            let endpoint = context.lock().unwrap_or_else(|e| e.into_inner());
            (endpoint.clone(), generation.load(Ordering::SeqCst))
        };
        worker
            .deliveries
            .refresh(epoch, &mods, keys.as_ref(), limit, &guard, now_ms());
        if let Some(socket) = socket {
            let library = control(&runtime, bridge::cinnabar_mods(&socket))
                .filter(|library| bridge::valid_cinnabar_xuid(&library.xuid));
            if let Some(library) = &library {
                guard.account(epoch, library.xuid.clone());
            }
            worker.refresh_due(library.as_ref(), &now_ms, &notify);
            if let Some(library) = library
                && library.mods.len() <= bridge::contract::CINNABAR_MODS_MAX_ENTRIES
            {
                worker.pass(
                    epoch,
                    &library,
                    |selected| {
                        control(&runtime, bridge::cinnabar_mods(&socket)).is_some_and(|current| {
                            current.xuid == library.xuid && current.mods.contains(selected)
                        })
                    },
                    |selected| {
                        let _ = control(&runtime, bridge::remove_cinnabar_mod(&socket, selected));
                    },
                    &now_ms,
                    &notify,
                );
            }
        }
        if let Err(RecvTimeoutError::Disconnected) = incoming.recv_timeout(Duration::from_secs(5)) {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
