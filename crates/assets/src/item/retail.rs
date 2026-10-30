//! The independent retail identity inventory used for item descriptor admission.

use crate::facts::LEGACY_ITEM_ALIASES;
use std::{collections::BTreeSet, sync::OnceLock};

/// The existing protocol inventory included in descriptor carrier fingerprints.
pub const SOURCE_BYTES: &[u8] = include_bytes!("../../../protocol/data/retail_items_1_26_60.tsv");
/// Logical source identity retained in an entity carrier.
pub const SOURCE_PATH: &str = "registry/retail-items-v2225.tsv";
/// Checkout input read by the entity carrier recipe.
pub const INPUT_PATH: &str = "crates/protocol/data/retail_items_1_26_60.tsv";

/// Reads the separately maintained identity inventory once, outside rendering paths.
pub fn identifiers() -> &'static BTreeSet<&'static str> {
    static IDENTIFIERS: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    IDENTIFIERS.get_or_init(|| {
        let rows = std::str::from_utf8(SOURCE_BYTES).expect("retail inventory is UTF-8");
        let identifiers = rows
            .lines()
            .filter_map(|row| row.split('\t').nth(1))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            identifiers.len(),
            rows.lines().count(),
            "retail identities are unique"
        );
        identifiers
    })
}

/// Resolves a namespaced descriptor name to its retail identity through verified aliases.
/// Descriptor names that are neither aliases nor retail identities stay unresolved.
#[must_use]
pub fn canonical_identifier(name: &str) -> Option<&'static str> {
    identifiers().get(name).copied().or_else(|| {
        LEGACY_ITEM_ALIASES
            .iter()
            .find(|(alias, _)| *alias == name)
            .and_then(|(_, target)| identifiers().get(target).copied())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_aliases_name_only_retired_names_and_resolve_to_retail_identities() {
        for (alias, target) in LEGACY_ITEM_ALIASES {
            assert!(
                !identifiers().contains(alias),
                "{alias} is a retail identity"
            );
            assert_eq!(canonical_identifier(alias), Some(*target));
        }
        assert_eq!(canonical_identifier("minecraft:bow"), Some("minecraft:bow"));
        assert_eq!(canonical_identifier("minecraft:carrotonastick"), None);
    }
}
