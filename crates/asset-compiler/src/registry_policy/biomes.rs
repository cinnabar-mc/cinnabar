use super::{
    Facts, Result, check, digest,
    manifest::{self, Output},
    read,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

#[derive(Deserialize, Serialize)]
struct Manifest {
    schema: String,
    game_version: String,
    protocol: u32,
    sources: Sources,
    allowlist: Allowlist,
    projection: Projection,
    output: Output,
}
#[derive(Deserialize, Serialize)]
struct Sources {
    shared_catalog: Catalog,
}
#[derive(Deserialize, Serialize)]
struct Catalog {
    module: String,
    source_lock_sha256: String,
    biome_count: usize,
}
#[derive(Deserialize, Serialize)]
struct Allowlist {
    path: String,
    sha256: String,
    count: usize,
}
#[derive(Deserialize, Serialize)]
struct Projection {
    retained: usize,
    ignored_count: usize,
    ignored_fingerprint: String,
}

/// Preserves source numeric biome identities while applying the retail name policy.
pub(super) fn compile(
    root: &Path,
    facts: &Facts,
    allowlist: &Path,
    out: &Path,
    report: &Path,
) -> Result<()> {
    let mut manifest: Manifest =
        serde_json::from_slice(&read(&root.join("assets/biome-projection-v2225.json"))?)?;
    check(
        facts.protocol == manifest.protocol
            && facts.game_version == manifest.game_version
            && facts.source_lock_sha256 == manifest.sources.shared_catalog.source_lock_sha256,
        "biome source target mismatch",
    )?;
    let data = read(allowlist)?;
    check(
        digest(&data) == manifest.allowlist.sha256,
        "biome allowlist hash mismatch",
    )?;
    let lines: Vec<_> = std::str::from_utf8(&data)?.lines().collect();
    check(
        lines.len() == manifest.allowlist.count
            && lines.windows(2).all(|p| p[0] < p[1])
            && lines
                .iter()
                .all(|s| s.starts_with("minecraft:") && s.len() <= assets::MAX_BIOME_NAME_BYTES),
        "invalid retail biome allowlist",
    )?;
    let allowed: BTreeSet<_> = lines.iter().copied().collect();
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut retained = Vec::new();
    let mut ignored = Vec::new();
    for b in &facts.biomes {
        check(
            b.has_id
                && (0..=u16::MAX as i32).contains(&b.id)
                && b.name.starts_with("minecraft:")
                && b.name.len() <= assets::MAX_BIOME_NAME_BYTES
                && ids.insert(b.id)
                && names.insert(b.name.as_str()),
            "invalid or duplicated source biome identity",
        )?;
        if allowed.contains(b.name.as_str()) {
            retained.push(b)
        } else {
            ignored.push(b)
        }
    }
    check(
        retained.len() == allowed.len(),
        "biome projection missing retail names",
    )?;
    retained.sort_by_key(|b| b.id);
    ignored.sort_by_key(|b| b.id);
    let mut hash = Sha256::new();
    for b in &ignored {
        hash.update((b.id as u32).to_le_bytes());
        hash.update((b.name.len() as u16).to_le_bytes());
        hash.update(b.name.as_bytes());
    }
    let mut bytes = assets::BIOME_REGISTRY_MAGIC.to_vec();
    bytes.extend_from_slice(&(retained.len() as u32).to_le_bytes());
    for b in &retained {
        bytes.extend_from_slice(&(b.id as u32).to_le_bytes());
        bytes.extend_from_slice(&(b.name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(b.name.as_bytes());
    }
    assets::read_biome_registry(&bytes)?;
    manifest.sources.shared_catalog.biome_count = facts.biomes.len();
    manifest.projection = Projection {
        retained: retained.len(),
        ignored_count: ignored.len(),
        ignored_fingerprint: format!("{:x}", hash.finalize()),
    };
    manifest.output.sha256 = digest(&bytes);
    manifest::write_blob(out, &bytes, false)?;
    manifest::write_json(report, &manifest)
}
