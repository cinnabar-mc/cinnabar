//! Translation of Java Edition resource packs into Bedrock resource packs.
//!
//! [`find_root`] recognizes a Java pack among archive paths and [`translate`] rewrites its
//! textures into a Bedrock pack with a generated manifest. Both editions' layouts are supported:
//! legacy 1.8-1.12 packs and flattened 1.13+ packs, including the 1.20.2+ HUD sprites and
//! 1.21.2+ equipment folders. Block and item textures are renamed to their Bedrock paths, armour
//! layers take Bedrock names, potions are pre-coloured, clock and compass frames are stitched into
//! atlases, the HUD atlases are cut into Bedrock's individual textures and an OptiFine sky becomes
//! the overworld cube map. Files Bedrock cannot use are ignored.
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
}

/// Name of the file that marks a Java pack's root.
const JAVA_MARKER: &str = "pack.mcmeta";

/// Finds the root of a Java pack among archive `paths`: the shallowest folder holding
/// `pack.mcmeta` or, failing that, `assets/minecraft/`. Returns `""` for the archive root or a
/// folder prefix ending in `/`, and `None` when the paths hold no Java pack.
pub fn find_root<'a>(paths: impl IntoIterator<Item = &'a str> + Clone) -> Option<String> {
    let shallowest = |roots: &mut dyn Iterator<Item = &'a str>| {
        roots
            .min_by_key(|root| (root.matches('/').count(), *root))
            .map(str::to_owned)
    };
    shallowest(&mut paths.clone().into_iter().filter_map(|path| {
        let root = path.strip_suffix(JAVA_MARKER)?;
        (root.is_empty() || root.ends_with('/')).then_some(root)
    }))
    .or_else(|| {
        shallowest(&mut paths.into_iter().filter_map(|path| {
            let start = path.find(meta::ASSETS)?;
            let root = &path[..start];
            (root.is_empty() || root.ends_with('/')).then_some(root)
        }))
    })
}

/// Translates the Java pack whose files are `paths` (relative to its root, as returned by
/// [`find_root`]) into a Bedrock pack named `name`. `read` returns a file's bytes, or `None` when
/// it cannot be read within the caller's limits; such files are counted in
/// [`Translation::skipped`]. Files are read sequentially on the calling thread and decoded in
/// parallel. The manifest identity is derived from the content read, so translating the same
/// pack twice yields the same pack identity.
pub fn translate(
    paths: &[String],
    read: impl FnMut(&str) -> Option<Vec<u8>>,
    name: &str,
) -> Result<Translation, TranslateError> {
    translate::translate(paths, read, name)
}

#[cfg(test)]
mod tests;
