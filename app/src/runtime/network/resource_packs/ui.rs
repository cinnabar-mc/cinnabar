//! Pack UI definitions follow the index's paths, including custom extensions.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::SPLASH_RESOURCE_PATH;
use resource_pack::LayeredPackView;

use super::ServerUiPack;
use ui_screens::menu_renderers::splashes::{MAX_SPLASH_SOURCE_BYTES, SplashSource};

/// Bound on splash text decoded across the whole stack.
const MAX_SPLASH_STACK_BYTES: usize = 16 * 1024 * 1024;
use json_ui::UI_DEFS_PATH as INDEX;
/// Indexes and definitions are read under the subscriber per-file allowance only:
/// the game's definition loader applies no stack-wide byte budget, so a valid
/// document is never dropped because earlier ones were large. Each layer reads a
/// stored entry once and every spelling of it shares those bytes, so retained
/// bytes stay within the stack's admitted entries.
const MAX_DEFINITION_BYTES: u64 = resource_pack::MAX_FILE_BYTES;

/// Bound on the bytes the catalog parses again for indexed spellings of an entry
/// it already loads (paths that differ from it only in letter case).
///
/// Each stored entry's first indexed spelling always loads, so no stored document
/// is ever dropped. Later spellings share its bytes but each one is parsed and
/// merged on its own, so without a bound a few kilobytes of index could repeat a
/// large document thousands of times. Version-matched Bedrock on a case-sensitive
/// file system does not load a spelling that differs from the stored name at all.
pub(super) const MAX_SPELLING_BYTES: usize = 16 * 1024 * 1024;

/// Collects indexed definitions and records dependencies for lazily loaded artwork.
pub(super) fn collect_server_ui(view: &LayeredPackView) -> Option<Arc<ServerUiPack>> {
    let mut splash_bytes = 0usize;
    let mut spelling_bytes = 0usize;
    let mut skipped_spellings = 0usize;
    let mut declared = BTreeSet::new();
    for index in view.read_layers_capped(INDEX, MAX_DEFINITION_BYTES) {
        if let Ok(paths) = json_ui::Catalog::declared_paths(&index) {
            declared.extend(paths);
        }
    }
    let mut pack = ServerUiPack {
        item_glint: pack_runtime::pack_textures::decode_pack_texture(
            view,
            assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER,
        )
        .and_then(|image| {
            render_model::UiTexturePage::owned([image.width, image.height], image.rgba8.into()).ok()
        })
        .map(Arc::new),
        splashes: view
            .read_layers_capped(SPLASH_RESOURCE_PATH, MAX_SPLASH_SOURCE_BYTES)
            .filter_map(|bytes| {
                splash_bytes += bytes.len();
                (splash_bytes <= MAX_SPLASH_STACK_BYTES)
                    .then(|| pack_runtime::pack_textures::parse_pack_json(&bytes))
                    .flatten()
                    .and_then(|root| SplashSource::from_json(&root))
            })
            .collect::<Vec<_>>()
            .into(),
        ..Default::default()
    };
    for layer in view.layers() {
        let paths: BTreeSet<_> = layer
            .files_under("ui/")
            .iter()
            .filter(|path| path.ends_with(".json"))
            .map(|path| (*path).to_owned())
            .chain(declared.iter().cloned())
            .collect();
        // Every path keeps its own catalog identity (what indexes list and what
        // namespace inheritance matches), while spellings that resolve to one
        // stored entry share a single read of its bytes.
        let mut reads: BTreeMap<&str, Option<Arc<[u8]>>> = BTreeMap::new();
        // Entries whose free spelling is an indexed path other than the stored name.
        let mut respelled = BTreeSet::new();
        let mut files = Vec::new();
        for path in paths {
            let Some(stored) = layer.resolved_path(&path, MAX_DEFINITION_BYTES) else {
                continue;
            };
            let bytes = reads.entry(stored).or_insert_with(|| {
                layer
                    .read_file_with_limit(&path, MAX_DEFINITION_BYTES)
                    .ok()
                    .flatten()
                    .map(Arc::from)
            });
            let Some(bytes) = bytes else {
                continue;
            };
            // The catalog parses only indexed paths. The stored name is the free
            // spelling when indexed, else the first indexed spelling is.
            let repeat = declared.contains(&path)
                && path != stored
                && (declared.contains(stored) || !respelled.insert(stored));
            if repeat {
                if spelling_bytes.saturating_add(bytes.len()) > MAX_SPELLING_BYTES {
                    skipped_spellings += 1;
                    continue;
                }
                spelling_bytes += bytes.len();
            }
            files.push((path, Arc::clone(bytes)));
        }
        pack.ui_layers.push(files);
    }
    if skipped_spellings > 0 {
        bevy::log::warn!(
            skipped = skipped_spellings,
            "server UI skipped repeated letter-case spellings of loaded definitions"
        );
    }
    // Art may live anywhere in the admitted archive. Its image and sidecar
    // bytes are consumed later by the renderer, so record them for reloads.
    view.track_contents("textures/");
    let mut has_images = false;
    for path in view.list_with_suffixes("", ServerUiPack::image_extensions()) {
        if ServerUiPack::is_image_path(path) {
            has_images = true;
            view.track_contents(path);
            if let Some((stem, _)) = path.rsplit_once('.') {
                view.track_contents(&format!("{stem}.json"));
            }
        }
    }
    if pack.is_empty() && !has_images && view.list("textures/").is_empty() {
        return None;
    }
    pack.view = Some(view.clone());
    Some(Arc::new(pack))
}
