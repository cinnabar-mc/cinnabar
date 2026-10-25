//! One cancellable download job per granted server session, away from rendering.

use crate::{
    bundle::VerifiedBundle, cache::BundleCache, negotiation::Grant, policy::MAX_EXPANDED_BYTES,
};
use anyhow::{Result, ensure};
use std::{path::PathBuf, sync::mpsc};
use tokio::sync::watch;

pub struct Download {
    cancel: watch::Sender<bool>,
    result: std::sync::Mutex<mpsc::Receiver<Result<Vec<VerifiedBundle>>>>,
}

impl Download {
    /// Sequential downloads bound aggregate connections, archive bytes and expanded data.
    pub fn start(grant: Grant, cache_root: PathBuf) -> Result<Self> {
        let (cancel, mut cancelled) = watch::channel(false);
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("experience-download".into())
            .spawn(move || {
                let work = (|| -> Result<Vec<VerifiedBundle>> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?;
                    runtime.block_on(async {
                        let cache = BundleCache::open_wait(
                            &cache_root,
                            &mut cancelled,
                            tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                        )
                        .await?;
                        let mut bundles = Vec::new();
                        let mut expanded = 0u64;
                        for offer in &grant.offer.offer.packages {
                            ensure!(!*cancelled.borrow(), "download cancelled");
                            let bytes = if let Some(bytes) = cache.read(&offer.digest)? {
                                bytes
                            } else {
                                if let Some(error) = unseeded(offer, &cache_root) {
                                    return Err(error);
                                }
                                let download = crate::fetch::fetch(
                                    &offer.url,
                                    &grant.offer.offer.scope.origins,
                                    offer.bytes as usize,
                                    None,
                                );
                                let bytes = tokio::select! {
                                    biased;
                                    _ = cancelled.changed() => anyhow::bail!("download cancelled"),
                                    result = download => result?,
                                };
                                ensure!(!*cancelled.borrow(), "download cancelled");
                                cache.publish(&offer.digest, &bytes)?;
                                bytes
                            };
                            let bundle = VerifiedBundle::read(
                                &bytes,
                                offer,
                                &grant.offer.offer.scope,
                                MAX_EXPANDED_BYTES - expanded,
                            )?;
                            expanded = expanded
                                .checked_add(bundle.expanded_bytes())
                                .ok_or_else(|| anyhow::anyhow!("expanded size overflow"))?;
                            ensure!(
                                expanded <= MAX_EXPANDED_BYTES,
                                "aggregate assets exceed limit"
                            );
                            bundles.push(bundle);
                        }
                        Ok(bundles)
                    })
                })();
                let _ = sender.send(work);
            })?;
        Ok(Self {
            cancel,
            result: std::sync::Mutex::new(result),
        })
    }

    /// Polls without waiting on network, hashing or archive decoding.
    pub fn poll(&self) -> Option<Result<Vec<VerifiedBundle>>> {
        match self.result.lock().ok()?.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("download worker stopped")))
            }
        }
    }
}

impl Drop for Download {
    /// Cancels the current request; late results cannot reach a replacement session.
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

/// The error for an uncached package whose offer names a reserved `.invalid` host (RFC 6761),
/// which never resolves: local servers deliver bundles only through the digest cache, so the
/// bundle has to be seeded there first. Other offers download as usual.
fn unseeded(
    offer: &crate::manifest::PackageOffer,
    cache_root: &std::path::Path,
) -> Option<anyhow::Error> {
    let url = url::Url::parse(&offer.url).ok()?;
    crate::manifest::reserved_origin(&url.origin().ascii_serialization()).then(|| {
        anyhow::anyhow!(
            "bundle {} ({}) is not in the bundle cache at {}, and its offer names {}, a \
             reserved host that never resolves; seed the cache first with `cinnabar-cxb \
             seed-cache` (the material-fx tool does this with `--user-data <client user data \
             directory>`)",
            offer.id,
            offer.digest,
            cache_root.display(),
            offer.url
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(url: &str) -> crate::manifest::PackageOffer {
        crate::manifest::PackageOffer {
            id: "materialfx".into(),
            publisher_key: String::new(),
            digest: "abc".into(),
            bytes: 1,
            url: url.into(),
        }
    }

    #[test]
    fn an_unseeded_reserved_host_bundle_names_the_seeding_step_instead_of_resolving() {
        let root = std::path::Path::new("/data/cache");
        let error = unseeded(&offer("https://cxb.invalid/abc.cxb"), root)
            .unwrap()
            .to_string();
        assert!(
            error.contains("materialfx") && error.contains("seed-cache"),
            "{error}"
        );
        assert!(error.contains("/data/cache"), "{error}");
        assert!(unseeded(&offer("https://example.com/abc.cxb"), root).is_none());
        assert!(unseeded(&offer("https://invalid.example/abc.cxb"), root).is_none());
    }
}
