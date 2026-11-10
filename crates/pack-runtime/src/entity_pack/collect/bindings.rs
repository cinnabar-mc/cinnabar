//! Collects the winning material binding document and records invalid input.

use resource_pack::LayeredPackView;

use super::CollectedFile;
use crate::pack_textures::parse_pack_json;

/// The winning `cinnabar/material-bindings.json`, canonicalised. It shadows lower copies even
/// when invalid; the compiler then ignores it and the reason is logged here.
/// Also says whether the winning copy is invalid: not JSON, or rejected by the extension's
/// validation, in which case the client ignores it whole.
pub(super) fn collect(view: &LayeredPackView) -> (Option<CollectedFile>, bool) {
    let path = pack_entity_compiler::MATERIAL_BINDINGS_PATH;
    let Some(bytes) = view.read(path) else {
        return (None, false);
    };
    let Some(document) = parse_pack_json(&bytes) else {
        tracing::warn!("ignoring the pack's material bindings: not JSON");
        return (None, true);
    };
    let invalid = match pack_entity_compiler::validate_material_bindings(&document) {
        Ok(()) => false,
        Err(reason) => {
            tracing::warn!(%reason, "ignoring the pack's material bindings");
            true
        }
    };
    let file = serde_json::to_vec(&document)
        .ok()
        .map(|bytes| (path.into(), bytes));
    (file, invalid)
}
