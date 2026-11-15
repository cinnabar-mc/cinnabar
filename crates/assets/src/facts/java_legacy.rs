//! The Java Edition 1.7.10 block behind a Bedrock block state.
//!
//! The core translates a Java 1.7.10 server's blocks to Bedrock states with
//! `core/javaedition/legacy/blocks.tsv`, and gives fences, panes, walls and stairs the
//! connection variants listed in `core/javaedition/translate/connected_states.tsv`. Java
//! physics on such a session needs the block the server sent, so this module inverts both
//! tables by network hash.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `id`, `metadata`, state, hash per row.
const BLOCKS: &str = include_str!("../../../../core/javaedition/legacy/blocks.tsv");
/// State, hash, `id`, `metadata` per row.
const CONNECTED: &str = include_str!("../../../../core/javaedition/translate/connected_states.tsv");

/// A block by its 1.7.10 numeric id and metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JavaLegacyBlock {
    pub id: u16,
    pub metadata: u8,
}

/// The 1.7.10 block the translator maps to the Bedrock state with this network hash, or
/// `None` for states it never produces. Where several blocks share a state, the lowest id
/// and metadata is returned.
#[must_use]
pub fn java_legacy_block(network_hash: u32) -> Option<JavaLegacyBlock> {
    static BY_HASH: OnceLock<HashMap<u32, JavaLegacyBlock>> = OnceLock::new();
    BY_HASH.get_or_init(parse).get(&network_hash).copied()
}

/// Every state hash the tables name, with the block behind it.
pub fn java_legacy_states() -> impl Iterator<Item = (u32, JavaLegacyBlock)> {
    static ROWS: OnceLock<Vec<(u32, JavaLegacyBlock)>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let mut rows: Vec<_> = parse().into_iter().collect();
        rows.sort_unstable_by_key(|(hash, _)| *hash);
        rows
    })
    .iter()
    .copied()
}

/// Reads both tables; both list rows from the lowest id and metadata up, and the first block
/// to name a hash keeps it.
fn parse() -> HashMap<u32, JavaLegacyBlock> {
    let mut by_hash = HashMap::new();
    let rows = |table: &'static str| {
        table
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                assert_eq!(fields.len(), 4, "malformed Java block table row {line:?}");
                fields
            })
    };
    let number = |text: &str| text.parse::<u32>().expect("Java block table number");
    for fields in rows(BLOCKS) {
        let block = JavaLegacyBlock {
            id: number(fields[0]) as u16,
            metadata: number(fields[1]) as u8,
        };
        by_hash.entry(number(fields[3])).or_insert(block);
    }
    for fields in rows(CONNECTED) {
        let block = JavaLegacyBlock {
            id: number(fields[2]) as u16,
            metadata: number(fields[3]) as u8,
        };
        by_hash.entry(number(fields[1])).or_insert(block);
    }
    by_hash
}

/// `id`, damage, Bedrock item, Bedrock id, Bedrock metadata per row.
const ITEMS: &str = include_str!("../../../../core/javaedition/legacy/items.tsv");

/// The 1.7.10 item ids registered as `ItemSword`: wooden, stone, iron, golden and diamond.
const SWORD_IDS: [u16; 5] = [268, 272, 267, 283, 276];

/// Whether the Bedrock item `identifier` is one the translator sends for a 1.7.10 sword, which
/// blocks while the use button is held.
#[must_use]
pub fn java_legacy_sword(identifier: &str) -> bool {
    java_legacy_swords().contains(&identifier)
}

/// The Bedrock items the translator sends for 1.7.10 swords.
fn java_legacy_swords() -> &'static [&'static str] {
    static SWORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    SWORDS.get_or_init(|| {
        let mut swords: Vec<&'static str> = ITEMS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let id: u16 = fields.next()?.parse().ok()?;
                let bedrock = fields.nth(1)?;
                SWORD_IDS.contains(&id).then_some(bedrock)
            })
            .collect();
        swords.sort_unstable();
        swords.dedup();
        swords
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_states_name_their_java_blocks() {
        // Stone is `1:0`; later metadata of the same id shares its state.
        assert_eq!(
            java_legacy_block(2_150_698_529),
            Some(JavaLegacyBlock { id: 1, metadata: 0 })
        );
        // Air.
        assert_eq!(
            java_legacy_block(3_690_217_760),
            Some(JavaLegacyBlock { id: 0, metadata: 0 })
        );
        assert_eq!(java_legacy_block(1), None);
    }

    #[test]
    fn connected_variants_name_the_block_they_vary() {
        // An oak fence joined to its east neighbour is still block 85.
        assert_eq!(
            java_legacy_block(3_720_363_220),
            Some(JavaLegacyBlock {
                id: 85,
                metadata: 0
            })
        );
        assert!(java_legacy_states().count() > 1500);
    }
}

#[cfg(test)]
mod sword_tests {
    /// Every 1.7.10 sword is in the translator's item table, each as its own Bedrock item.
    #[test]
    fn every_java_sword_has_a_bedrock_item() {
        assert_eq!(super::java_legacy_swords().len(), super::SWORD_IDS.len());
        assert!(super::java_legacy_sword("minecraft:diamond_sword"));
        assert!(!super::java_legacy_sword("minecraft:bow"));
    }
}
