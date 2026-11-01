//! Where Java Edition textures belong in a Bedrock pack.

use crate::meta::Layout;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

const FLATTENED_NAMES: &str = include_str!("../data/flattened-names.tsv");
const LEGACY_NAMES: &str = include_str!("../data/legacy-names.tsv");
const FLIPBOOKS: &str = include_str!("../data/flipbooks.txt");

/// Lines of a bundled data file that are neither blank nor `#` comments.
fn data_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// Parses a tab-separated `java<TAB>bedrock` table.
fn parse_table(text: &'static str) -> HashMap<&'static str, &'static str> {
    data_lines(text)
        .map(|line| {
            line.split_once('\t')
                .unwrap_or_else(|| panic!("name table line without a tab: {line}"))
        })
        .collect()
}

static FLATTENED: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| parse_table(FLATTENED_NAMES));
static LEGACY: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| parse_table(LEGACY_NAMES));
static FLIPBOOK_SET: LazyLock<HashSet<&str>> = LazyLock::new(|| data_lines(FLIPBOOKS).collect());

/// The Bedrock path, relative to `textures/` without extension, of a Java block texture named
/// `name` (without extension), such as `blocks/planks_oak`.
pub(crate) fn block_path(layout: Layout, name: &str) -> String {
    bedrock_path(layout, "block", "blocks", name)
}

/// The Bedrock path, relative to `textures/` without extension, of a Java item texture named
/// `name` (without extension), such as `items/apple_golden`.
pub(crate) fn item_path(layout: Layout, name: &str) -> String {
    bedrock_path(layout, "item", "items", name)
}

/// Looks `name` up in the table for `layout`, defaulting to the same name in `bedrock_dir`.
fn bedrock_path(layout: Layout, flattened_dir: &str, bedrock_dir: &str, name: &str) -> String {
    let (table, key) = match layout {
        Layout::Flattened => (&*FLATTENED, format!("{flattened_dir}/{name}")),
        Layout::Legacy => (&*LEGACY, format!("{bedrock_dir}/{name}")),
    };
    table.get(key.as_str()).map_or_else(
        || format!("{bedrock_dir}/{name}"),
        |path| (*path).to_owned(),
    )
}

/// Whether vanilla Bedrock animates the texture at `path` (relative to `textures/`, without
/// extension) as a vertical strip of frames.
pub(crate) fn is_flipbook(path: &str) -> bool {
    FLIPBOOK_SET.contains(path)
}

/// Java worn-armour layer names and the Bedrock names that replace them.
const ARMOR_NAMES: [(&str, &str); 15] = [
    ("chainmail_layer_1", "chain_1"),
    ("chainmail_layer_2", "chain_2"),
    ("copper_layer_1", "copper_1"),
    ("copper_layer_2", "copper_2"),
    ("diamond_layer_1", "diamond_1"),
    ("diamond_layer_2", "diamond_2"),
    ("gold_layer_1", "gold_1"),
    ("gold_layer_2", "gold_2"),
    ("iron_layer_1", "iron_1"),
    ("iron_layer_2", "iron_2"),
    ("leather_layer_1", "leather_1"),
    ("leather_layer_2", "leather_2"),
    ("netherite_layer_1", "netherite_1"),
    ("netherite_layer_2", "netherite_2"),
    ("turtle_layer_1", "turtle_1"),
];

/// The Bedrock file name of a worn-armour layer named `<material>_layer_<n>[_overlay].png`.
pub(crate) fn armor_name(name: &str) -> String {
    ARMOR_NAMES
        .iter()
        .find(|(java, _)| name.contains(java))
        .map_or_else(
            || name.to_owned(),
            |(java, bedrock)| name.replacen(java, bedrock, 1),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse_without_duplicate_java_paths() {
        for text in [FLATTENED_NAMES, LEGACY_NAMES] {
            let lines = data_lines(text).count();
            assert_eq!(parse_table(text).len(), lines, "duplicate Java path");
        }
        assert_eq!(data_lines(FLIPBOOKS).count(), FLIPBOOK_SET.len());
    }

    #[test]
    fn every_bedrock_path_is_a_texture_path() {
        for path in FLATTENED
            .values()
            .chain(LEGACY.values())
            .chain(FLIPBOOK_SET.iter())
        {
            assert!(
                [
                    "blocks/",
                    "items/",
                    "environment/",
                    "entity/",
                    "models/",
                    "particle/",
                    "ui/"
                ]
                .iter()
                .any(|prefix| path.starts_with(prefix)),
                "{path}"
            );
            assert!(!path.ends_with(".png"), "{path}");
        }
    }

    /// The pinned sample pack's `resource_pack` folder, or `None` with a missing-fixture notice.
    fn sample_pack() -> Option<std::path::PathBuf> {
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

    #[test]
    fn tables_match_the_pinned_sample_pack() {
        let Some(pack) = sample_pack() else {
            return;
        };
        for path in FLATTENED.values().chain(LEGACY.values()) {
            let texture = pack.join("textures").join(path);
            assert!(
                ["png", "tga"]
                    .iter()
                    .any(|extension| texture.with_extension(extension).is_file()),
                "{path} is not a sample-pack texture"
            );
        }
        let flipbooks: Vec<serde_json::Value> = pack_json::parse_jsonc(
            &std::fs::read(pack.join("textures/flipbook_textures.json")).unwrap(),
        )
        .unwrap();
        let vanilla: HashSet<&str> = flipbooks
            .iter()
            .filter_map(|entry| {
                entry["flipbook_texture"]
                    .as_str()?
                    .strip_prefix("textures/")
            })
            .collect();
        assert_eq!(*FLIPBOOK_SET, vanilla);
    }

    #[test]
    fn known_and_unknown_names_resolve_per_layout() {
        assert_eq!(
            block_path(Layout::Flattened, "acacia_planks"),
            "blocks/planks_acacia"
        );
        assert_eq!(block_path(Layout::Flattened, "stone"), "blocks/stone");
        assert_eq!(item_path(Layout::Legacy, "fish_cod_raw"), "items/fish_raw");
        assert_eq!(
            block_path(Layout::Legacy, "planks_oak"),
            "blocks/planks_oak"
        );
        assert!(is_flipbook("blocks/fire_0"));
        assert!(!is_flipbook("blocks/stone"));
    }

    #[test]
    fn armor_layers_take_bedrock_names() {
        assert_eq!(armor_name("chainmail_layer_2.png"), "chain_2.png");
        assert_eq!(
            armor_name("leather_layer_1_overlay.png"),
            "leather_1_overlay.png"
        );
        assert_eq!(armor_name("custom.png"), "custom.png");
    }
}
