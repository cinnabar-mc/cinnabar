//! Periodic compatible release selection over verified, account-owned receipts.

use super::*;
use launcher::mods::AvailableUpdate;

/// Low bits hold update preferences; higher bits distinguish successive authorizations.
const POLICY_FLAGS: u64 = 3;

/// Native compatibility identity, supplied by the same backend as the official updater.
pub(crate) struct Host {
    pub revision: &'static str,
    pub api_revisions_json: &'static str,
}

/// Frame-thread preferences and offers; a change cancels already prepared updates.
#[derive(Default)]
pub(crate) struct Policy {
    authorization: Arc<AtomicU64>,
    manual: Mutex<HashSet<u64>>,
    offers: Mutex<Vec<AvailableUpdate>>,
}

impl Policy {
    /// Replaces preferences while preserving a manual request until its next worker pass.
    pub fn configure(
        &self,
        checks: bool,
        automatic: bool,
        manual: impl IntoIterator<Item = u64>,
    ) -> bool {
        let flags = u8::from(checks) | (u8::from(checks && automatic) << 1);
        let previous =
            self.authorization
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                    (current & POLICY_FLAGS != u64::from(flags))
                        .then(|| (current | POLICY_FLAGS).wrapping_add(1) | u64::from(flags))
                });
        let changed = previous.is_ok();
        if changed {
            self.offers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
        }
        let mut pending = self.manual.lock().unwrap_or_else(|e| e.into_inner());
        let before = pending.len();
        pending.extend(manual);
        changed || pending.len() != before
    }

    /// Reads preferences and their cancellation identity from one atomic value.
    fn snapshot(&self, after_read: impl FnOnce()) -> (u8, u64) {
        let authorization = self.authorization.load(Ordering::SeqCst);
        let flags = (authorization & POLICY_FLAGS) as u8;
        after_read();
        (flags, authorization)
    }

    /// Cancels pending update work and removes old-account presentation.
    pub fn clear(&self) {
        self.authorization
            .fetch_add(POLICY_FLAGS + 1, Ordering::SeqCst);
        self.manual
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.offers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Copies the bounded presentation snapshot without performing I/O on the frame thread.
    pub fn offers(&self) -> Vec<AvailableUpdate> {
        self.offers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// A presentation notice retains its signed digest so withdrawals also hide cached offers.
struct Offer {
    update: AvailableUpdate,
    sha256: String,
}

#[derive(Default)]
pub(super) struct Updates {
    epoch: u64,
    attempts: HashMap<PathBuf, Failure>,
    offers: HashMap<PathBuf, Offer>,
    receipts: Vec<Receipt>,
    sources: Vec<(u64, PathBuf)>,
    scan_at: u64,
}

impl Updates {
    /// Checks installed receipts once per refresh interval, backing off failures like deliveries.
    #[allow(
        clippy::too_many_arguments,
        reason = "binds update lookup to live account, native compatibility and preference fences"
    )]
    pub(super) fn pass(
        &mut self,
        worker: &Worker<'_>,
        epoch: u64,
        library: &CinnabarModLibrary,
        host: &Host,
        policy: &Policy,
        confirm: impl Fn() -> bool,
        now: &impl Fn() -> u64,
        notify: &impl Fn(String),
    ) {
        if self.epoch != epoch {
            *self = Self {
                epoch,
                ..Default::default()
            };
        }
        let (flags, authorization) = policy.snapshot(|| {});
        if flags & 1 == 0 {
            return;
        }
        let Some(keys) = worker.keys else {
            return;
        };
        let Ok(supported) = serde_json::from_str::<Vec<String>>(host.api_revisions_json) else {
            return;
        };
        if !revision_valid(host.revision)
            || supported.is_empty()
            || !supported.iter().all(|r| revision_valid(r))
        {
            return;
        }
        let manual = std::mem::take(&mut *policy.manual.lock().unwrap_or_else(|e| e.into_inner()));
        let installed = crate::mod_library::files::installed(worker.mods);
        let sources: Vec<_> = installed
            .iter()
            .map(|(item, path)| (item.id, path.clone()))
            .collect();
        if self.sources != sources || now() >= self.scan_at {
            self.receipts = storage::restore(worker.mods, keys, worker.limit, &worker.guard, now());
            self.sources = sources;
            self.scan_at = now().saturating_add(REFRESH_INTERVAL_MS);
        }
        for successor in &self.receipts {
            if successor.owner != library.xuid {
                continue;
            }
            if let Some(name) = &successor.migration {
                let previous = worker.mods.join(name);
                let known = self.receipts.iter().any(|old| {
                    old.owner == successor.owner
                        && old.selected.slug == successor.selected.slug
                        && storage::filename(&old.owner, &old.selected)
                            .is_ok_and(|old_name| old_name == *name)
                });
                if !previous.exists()
                    && (!known || storage::retire_receipt(&previous, &worker.guard).is_ok())
                    && let Ok(name) = storage::filename(&successor.owner, &successor.selected)
                {
                    let _ = storage::finish_migration(&worker.mods.join(name));
                }
            }
        }
        let receipts = &self.receipts;
        self.offers.retain(|path, offer| {
            installed.iter().any(|(_, current)| path == current)
                && worker.guard.allows(path, now())
                && worker.guard.permits(&library.xuid, &offer.sha256, now())
        });
        self.attempts
            .retain(|path, _| installed.iter().any(|(_, current)| path == current));
        for receipt in receipts {
            if receipt.owner != library.xuid {
                continue;
            }
            // Finish this release's predecessor before looking for yet another successor.
            if receipt
                .migration
                .as_ref()
                .is_some_and(|name| worker.mods.join(name).exists())
            {
                continue;
            }
            let Ok(name) = storage::filename(&receipt.owner, &receipt.selected) else {
                continue;
            };
            let path = worker.mods.join(name);
            let Some((item, _)) = installed.iter().find(|(_, current)| current == &path) else {
                continue;
            };
            // Only permission-staged successors block replacement; unfinished migrations resume.
            let successor = receipts.iter().find(|other| {
                other.owner == receipt.owner
                    && worker
                        .guard
                        .permits(&other.owner, &other.selected.sha256, now())
                    && other.selected.slug == receipt.selected.slug
                    && update_manifest::select::newer(
                        &other.selected.version,
                        &receipt.selected.version,
                    )
                    .unwrap_or(false)
                    && storage::filename(&other.owner, &other.selected)
                        .is_ok_and(|name| worker.mods.join(name).exists())
            });
            let resuming = successor.is_some_and(|other| {
                other.migration.as_deref() == path.file_name().and_then(|name| name.to_str())
            });
            if successor.is_some() && !resuming {
                self.offers.remove(&path);
                continue;
            }
            if !worker
                .guard
                .permits(&receipt.owner, &receipt.selected.sha256, now())
            {
                continue;
            }
            if !manual.contains(&item.id)
                && self
                    .attempts
                    .get(&path)
                    .is_some_and(|failure| now() < failure.retry_at)
            {
                continue;
            }
            if worker.generation.load(Ordering::SeqCst) != epoch {
                return;
            }
            let result = if let Some(successor) = successor.filter(|_| resuming) {
                successor
                    .release
                    .verify(keys, &successor.selected, worker.limit, now())
                    .map(|manifest| {
                        Some((
                            successor.release.clone(),
                            manifest,
                            successor.selected.clone(),
                        ))
                    })
            } else {
                worker.runtime.block_on(lookup(
                    worker.transport,
                    keys,
                    receipt,
                    host,
                    &supported,
                    worker.limit,
                    now(),
                ))
            };
            let mut queued = false;
            let result = result.and_then(|release| {
                self.offers.remove(&path);
                let Some((release, manifest, selected)) = release else {
                    return Ok(());
                };
                ensure!(
                    worker
                        .guard
                        .permits(&receipt.owner, &manifest.sha256, now()),
                    "Marketplace update is held."
                );
                self.offers.insert(
                    path.clone(),
                    Offer {
                        update: AvailableUpdate {
                            id: item.id,
                            version: manifest.version.clone(),
                            release_url: String::new(),
                        },
                        sha256: manifest.sha256.clone(),
                    },
                );
                if flags & 2 == 0 && !manual.contains(&item.id) {
                    return Ok(());
                }
                let mut prepared = worker.runtime.block_on(stage(
                    worker, epoch, receipt, release, manifest, selected, now,
                ))?;
                prepared.previous = Some(path.clone());
                prepared.authorization = Some((policy.authorization.clone(), authorization));
                prepared.receipt.consumed = library
                    .mods
                    .iter()
                    .filter(|selected| {
                        **selected == receipt.selected || receipt.consumed.contains(selected)
                    })
                    .cloned()
                    .collect();
                ensure!(
                    confirm() && policy.authorization.load(Ordering::SeqCst) == authorization,
                    "Marketplace update cancelled."
                );
                worker
                    .outgoing
                    .try_send(prepared)
                    .map_err(|_| anyhow::anyhow!("Marketplace installer queue is full."))?;
                queued = true;
                // Retry failed compatibility/publication handoffs; a successful successor is
                // recognized from its receipt on the next pass.
                Ok(())
            });
            let attempt = self.attempts.entry(path.clone()).or_insert(Failure {
                count: 0,
                retry_at: 0,
            });
            if let Err(error) = result {
                self.offers.remove(&path);
                attempt.count = attempt.count.saturating_add(1);
                attempt.retry_at = now().saturating_add(retry_delay(attempt.count));
                notify(format!(
                    "Marketplace update could not be prepared: {error:#}"
                ));
            } else if queued {
                attempt.count = attempt.count.saturating_add(1);
                attempt.retry_at = now().saturating_add(retry_delay(attempt.count));
            } else {
                attempt.count = 0;
                attempt.retry_at = now().saturating_add(REFRESH_INTERVAL_MS);
            }
        }
        if worker.generation.load(Ordering::SeqCst) == epoch
            && policy.authorization.load(Ordering::SeqCst) == authorization
        {
            *policy.offers.lock().unwrap_or_else(|e| e.into_inner()) = self
                .offers
                .values()
                .map(|offer| offer.update.clone())
                .collect();
        }
    }
}

/// Requires full native/API Git identities before constructing a compatibility query.
fn revision_valid(revision: &str) -> bool {
    revision.len() == 40
        && revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Fetches signed update metadata; absent or hidden listings make no local change.
async fn lookup(
    transport: &Transport,
    keys: &Keys,
    receipt: &Receipt,
    host: &Host,
    supported: &[String],
    limit: usize,
    now: u64,
) -> Result<Option<(SignedRelease, super::super::Manifest, CinnabarModEntry)>> {
    let mut url = transport.origin.clone();
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("Invalid marketplace origin."))?
        .extend(["v1", "mods", &receipt.selected.slug, "update"]);
    url.query_pairs_mut().extend_pairs([
        ("api_revisions", supported.join(",").as_str()),
        ("host_revision", host.revision),
        ("current_version", receipt.selected.version.as_str()),
        ("current_sha256", receipt.selected.sha256.as_str()),
    ]);
    let response = transport
        .client
        .get(url)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    if matches!(
        response.status(),
        reqwest::StatusCode::NO_CONTENT | reqwest::StatusCode::NOT_FOUND
    ) {
        return Ok(None);
    }
    let (body, _) = read_response(response, MAX_RESPONSE_BYTES).await?;
    #[derive(Deserialize)]
    struct Response {
        release: SignedRelease,
    }
    let release = serde_json::from_slice::<Response>(&body)?.release;
    let (manifest, selected) =
        release.verify_update(keys, &receipt.selected, supported, limit, now)?;
    Ok(Some((release, manifest, selected)))
}

/// Stages digest-bound bytes under the delivery worker's existing account and revocation fence.
async fn stage(
    worker: &Worker<'_>,
    epoch: u64,
    old: &Receipt,
    release: SignedRelease,
    manifest: super::super::Manifest,
    selected: CinnabarModEntry,
    now: &impl Fn() -> u64,
) -> Result<Downloaded> {
    let target = worker.mods.join(storage::filename(&old.owner, &selected)?);
    let bytes = if target.exists() {
        storage::read(&target, manifest.bytes as usize)?
    } else {
        fetch(
            worker.transport,
            &format!("/v1/mods/assets/{}", manifest.sha256),
            manifest.bytes as usize,
            asset_timeout(manifest.bytes),
        )
        .await?
        .0
    };
    super::super::verify::verify_bytes(&bytes, &manifest)?;
    ensure!(
        worker.generation.load(Ordering::SeqCst) == epoch
            && worker.guard.permits(&old.owner, &manifest.sha256, now()),
        "Marketplace update cancelled or held."
    );
    let mut file = tempfile::NamedTempFile::new_in(worker.mods)?;
    file.write_all(&bytes)?;
    Ok(Downloaded {
        file,
        manifest,
        receipt: Receipt {
            owner: old.owner.clone(),
            selected,
            release,
            consumed: Vec::new(),
            migration: None,
        },
        generation: worker.generation.clone(),
        epoch,
        guard: worker.guard.clone(),
        mods: worker.mods.into(),
        previous: None,
        authorization: None,
    })
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
