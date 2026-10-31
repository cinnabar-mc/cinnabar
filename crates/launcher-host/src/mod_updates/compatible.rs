//! Stable-API catalogue selection; this path never falls back to a latest GitHub release.

use super::{Candidate, download::Asset, worker::Release};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;

const HOST_REPOSITORY: &str = "https://github.com/cinnabar-mc/cinnabar.git";
const MAX_METADATA_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct Record {
    manifest: Manifest,
    download_url: String,
}
#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    package: String,
    version: String,
    revision: String,
    host: Host,
    artifact: Artifact,
}
#[derive(Deserialize)]
struct Host {
    repository: String,
    revision: String,
    api_revision: String,
}
#[derive(Deserialize)]
struct Artifact {
    filename: String,
    size: u64,
    sha256: String,
}

/// Requires a stamped native revision and sends no account or publisher credentials.
pub(super) async fn fetch(
    client: &reqwest::Client,
    candidate: &Candidate,
    host: &str,
    api_revisions_json: &str,
) -> Result<Option<Release>> {
    let api_revisions: Vec<String> = serde_json::from_str(api_revisions_json)?;
    if !full_revision(host)
        || api_revisions.is_empty()
        || api_revisions.len() > 32
        || !api_revisions.iter().all(|revision| full_revision(revision))
    {
        return Ok(None);
    }
    let identity = candidate
        .revision_update
        .as_ref()
        .context("missing update identity")?;
    let mut url = url::Url::parse(bridge::contract::CINNABAR_API_BASE_URL)?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid API base"))?
        .extend(["v1", "mod", "update"]);
    let supported = api_revisions.join(",");
    url.query_pairs_mut().extend_pairs([
        ("host_revision", host),
        ("api_revisions", supported.as_str()),
        (
            "current_version",
            candidate
                .version
                .strip_prefix('v')
                .unwrap_or(&candidate.version),
        ),
        ("current_revision", &identity.revision),
    ]);
    let mut response = client.get(url).send().await?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "compatible update lookup failed"
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_METADATA_BYTES as u64),
        "update metadata exceeds limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_METADATA_BYTES,
            "update metadata exceeds limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    select(&bytes, candidate, &api_revisions).map(Some)
}

/// Checks the service response against local identity, host compatibility and trusted source.
fn select(bytes: &[u8], candidate: &Candidate, api_revisions: &[String]) -> Result<Release> {
    let record: Record = serde_json::from_slice(bytes)?;
    let m = record.manifest;
    let old = candidate
        .revision_update
        .as_ref()
        .context("missing update identity")?;
    ensure!(
        m.schema_version == 1 && m.package == old.package,
        "unexpected mod identity"
    );
    ensure!(
        m.host.repository == HOST_REPOSITORY
            && full_revision(&m.host.revision)
            && api_revisions.contains(&m.host.api_revision),
        "unsupported mod API revision"
    );
    ensure!(
        full_revision(&m.revision) && m.revision != old.revision,
        "invalid mod revision"
    );
    ensure!(
        m.artifact.filename == old.source.asset(),
        "unexpected release asset"
    );
    let expected = format!(
        "{}/v1/mod/assets/{}",
        bridge::contract::CINNABAR_API_BASE_URL.trim_end_matches('/'),
        m.artifact.sha256
    );
    ensure!(
        record.download_url == expected,
        "unexpected release download URL"
    );
    let mut identity = old.clone();
    identity.revision = m.revision;
    identity.api_revision = m.host.api_revision;
    let asset = Asset {
        id: 0,
        name: m.artifact.filename,
        size: m.artifact.size,
        digest: Some(format!("sha256:{}", m.artifact.sha256)),
        url: Some(record.download_url),
        revision_update: Some(identity),
    };
    ensure!(
        super::download::select(std::slice::from_ref(&asset), old.source.asset(), usize::MAX)
            .is_some(),
        "invalid release size or digest"
    );
    Ok(Release {
        tag_name: format!("v{}", m.version),
        draft: false,
        prerelease: false,
        assets: vec![asset],
    })
}

/// Only full lowercase Git object identities can establish compatibility.
fn full_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic catalogue record, independent of any particular mod feature.
    fn fixture() -> (Candidate, serde_json::Value, String) {
        let host = "a".repeat(40);
        let identity = ui::mod_metadata::RevisionModUpdate::parse(
            serde_json::json!({
                "source": "https://github.com/example/mod/releases/latest/download/mod.wasm",
                "package": "example-mod", "api_revision": "a".repeat(40), "revision": "b".repeat(40),
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
        let candidate = Candidate {
            id: 1,
            repository: "example/mod".into(),
            version: "1.0.0".into(),
            asset: Some("mod.wasm".into()),
            revision_update: Some(identity),
        };
        let record = serde_json::json!({"manifest": {
            "schema_version": 1, "package": "example-mod", "version": "1.1.0", "revision": "c".repeat(40),
            "host": {"repository": HOST_REPOSITORY, "revision": "e".repeat(40), "api_revision": host},
            "artifact": {"filename": "mod.wasm", "size": 8, "sha256": "d".repeat(64)}
        }, "download_url": format!("{}/v1/mod/assets/{}", bridge::contract::CINNABAR_API_BASE_URL, "d".repeat(64))});
        (candidate, record, host)
    }

    #[test]
    fn compatible_response_is_bound_to_host_source_package_and_digest() {
        let (candidate, record, host) = fixture();
        let selected = select(
            &serde_json::to_vec(&record).unwrap(),
            &candidate,
            std::slice::from_ref(&host),
        )
        .unwrap();
        assert!(
            select(
                &serde_json::to_vec(&record).unwrap(),
                &candidate,
                &["f".repeat(40), host.clone()],
            )
            .is_ok()
        );
        assert_eq!(
            selected.assets[0]
                .revision_update
                .as_ref()
                .unwrap()
                .revision,
            "c".repeat(40)
        );
        for pointer in [
            "/manifest/package",
            "/manifest/revision",
            "/manifest/host/repository",
            "/manifest/host/api_revision",
            "/manifest/host/revision",
            "/manifest/artifact/filename",
            "/manifest/artifact/sha256",
            "/download_url",
        ] {
            let mut bad = record.clone();
            *bad.pointer_mut(pointer).unwrap() = serde_json::json!("unexpected");
            assert!(
                select(
                    &serde_json::to_vec(&bad).unwrap(),
                    &candidate,
                    std::slice::from_ref(&host)
                )
                .is_err(),
                "{pointer}"
            );
        }
    }
}
