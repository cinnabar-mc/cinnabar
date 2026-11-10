//! Translation of Java Edition resource packs into Bedrock resource packs.
//!
//! [`find_root`] recognizes a Java pack among archive paths and [`translate`] rewrites its
//! textures into a Bedrock pack with a generated manifest. Both Java layouts are supported:
//! legacy pre-1.13 packs, 1.7 and 1.8 included, and flattened 1.13+ packs with the 1.20.2+ HUD
//! sprites and 1.21.2+ equipment folders. Block and item textures are renamed to their Bedrock
//! paths, armour layers take Bedrock names, potions are pre-coloured, clock and compass frames or
//! pre-1.9 strips become the Bedrock atlases, the HUD atlases are cut into Bedrock's individual
//! textures and an OptiFine sky becomes the overworld cube map. Files Bedrock cannot use are
//! ignored.
//!
//! The mappings follow portmypack (<https://github.com/RestartFU/portmypack>), whose tables were
//! derived by matching Java Edition textures against Mojang/bedrock-samples.

mod hud;
mod items;
mod manifest;
mod meta;
mod names;
mod sky;
mod texture;
mod tga;
mod translate;

pub use manifest::MIN_ENGINE_VERSION;
pub use meta::Layout;

/// The files of a Bedrock pack translated from a Java pack.
#[derive(Debug)]
pub struct Translation {
    /// Pack files keyed by their path, `manifest.json` included, sorted by path.
    pub files: Vec<(String, Vec<u8>)>,
    /// The Java layout the textures were read from.
    pub layout: Layout,
    /// Source files that were unreadable or not valid PNGs and were left out.
    pub skipped: usize,
}

/// Why a Java pack produced no Bedrock pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TranslateError {
    #[error("Java Edition pack has no textures Bedrock can use")]
    NothingToTranslate,
    #[error("Java Edition pack needs more image memory to translate than one import allows")]
    TooLarge,
}

/// Name of the file that marks a Java pack's root.
const JAVA_MARKER: &str = "pack.mcmeta";

/// Finds the root of a Java pack among archive `paths`: the shallowest folder holding
/// `assets/minecraft/` or, failing that, the shallowest holding `pack.mcmeta`, so a data pack's
/// `pack.mcmeta` beside or inside a resource pack never wins. Returns `""` for the archive root or
/// a folder prefix ending in `/`, and `None` when the paths hold no Java pack.
pub fn find_root<'a>(paths: impl IntoIterator<Item = &'a str> + Clone) -> Option<String> {
    let shallowest = |roots: &mut dyn Iterator<Item = &'a str>| {
        roots
            .min_by_key(|root| (root.matches('/').count(), *root))
            .map(str::to_owned)
    };
    let folder_root = |root: &'a str| (root.is_empty() || root.ends_with('/')).then_some(root);
    shallowest(
        &mut paths
            .clone()
            .into_iter()
            .filter_map(|path| folder_root(&path[..path.find(meta::ASSETS)?])),
    )
    .or_else(|| {
        shallowest(
            &mut paths
                .into_iter()
                .filter_map(|path| folder_root(path.strip_suffix(JAVA_MARKER)?)),
        )
    })
}

/// Translates the Java pack whose files are `paths` (relative to its root, as returned by
/// [`find_root`]) into a Bedrock pack named `name`. `read` returns a file's bytes, or `None` when
/// it cannot be read within the caller's limits; such files, and generated textures whose encoding
/// exceeds `max_texture_bytes` (the runtime's texture source limit), are counted in
/// [`Translation::skipped`]. Files are read sequentially on the calling thread and decoded in
/// parallel. The manifest identity is derived from the content read, so translating the same
/// pack twice yields the same pack identity.
pub fn translate(
    paths: &[String],
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    name: &str,
    max_texture_bytes: u64,
) -> Result<Translation, TranslateError> {
    translate::translate(paths, read, name, max_texture_bytes)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support {
    /// The pinned sample pack's `resource_pack` folder, or `None` with a missing-fixture notice.
    pub(crate) fn sample_pack() -> Option<std::path::PathBuf> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("assets/vanilla-source.json")).unwrap(),
        )
        .unwrap();
        let pack = root
            .join(manifest["cache_dir"].as_str().unwrap())
            .join("resource_pack");
        if pack.join("textures/flipbook_textures.json").is_file() {
            return Some(pack);
        }
        eprintln!(
            "missing fixture: pinned sample pack at {}; skipping (run `make vanilla-assets`)",
            pack.display()
        );
        None
    }
}
