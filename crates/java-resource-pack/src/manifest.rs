//! The Bedrock manifest written for a translated pack.

use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::{Builder, Uuid};

/// Manifest format written for translated packs. Bedrock 1.13 introduced format 2 and current
/// clients still accept it.
const FORMAT_VERSION: u32 = 2;
/// The first Bedrock release that reads format 2 manifests.
pub const MIN_ENGINE_VERSION: [u32; 3] = [1, 13, 0];
/// Description used when `pack.mcmeta` has none.
const DEFAULT_DESCRIPTION: &str = "Java Edition resource pack";

/// A UUID derived from `digest` and a purpose label, so re-importing the same Java pack keeps its
/// identity while its header and module stay distinct.
fn derived_uuid(digest: &[u8; 32], label: &str) -> Uuid {
    let hash = Sha256::new()
        .chain_update(digest)
        .chain_update(label.as_bytes())
        .finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    Builder::from_custom_bytes(bytes).into_uuid()
}

/// Serializes the manifest of a translated pack whose source content hashes to `digest`.
pub(crate) fn manifest(name: &str, description: &str, digest: &[u8; 32]) -> Vec<u8> {
    let description = if description.is_empty() {
        DEFAULT_DESCRIPTION
    } else {
        description
    };
    let manifest = json!({
        "format_version": FORMAT_VERSION,
        "header": {
            "description": description,
            "name": name,
            "uuid": derived_uuid(digest, "header").to_string(),
            "version": [1, 0, 0],
            "min_engine_version": MIN_ENGINE_VERSION,
        },
        "modules": [{
            "description": description,
            "type": "resources",
            "uuid": derived_uuid(digest, "resources").to_string(),
            "version": [1, 0, 0],
        }],
    });
    serde_json::to_vec_pretty(&manifest).expect("a JSON value always serializes")
}
