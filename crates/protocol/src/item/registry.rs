//! Item registry entries and the pinned retail registry.
use std::sync::Arc;

use super::ItemComponents;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ItemRegistryVersion {
    Legacy,
    DataDriven,
    None,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRegistryEntry {
    /// Client-facing components parsed once from the validated registry payload.
    pub components: Option<Arc<ItemComponents>>,
    pub identifier: Arc<str>,
    pub network_id: i32,
    pub component_based: bool,
    pub version: ItemRegistryVersion,
    pub component_digest: [u8; 32],
    /// Exact positive stack capacity retained from the negotiated component path.
    ///
    /// This is evidence only: it neither supplies a fallback nor binds a later
    /// inventory decision to the registry or session generation.
    pub negotiated_max_stack_size: Option<u8>,
    /// Whether the component payload is the exact canonical empty compound.
    pub canonical_empty_component_data: bool,
    /// Item tags a component-based entry declares for itself.
    pub item_tags: Arc<[Arc<str>]>,
}

/// Returns the retail-positive item registry for the pinned Bedrock protocol.
///
/// The compact table contains only entries referenced by the default creative
/// inventory. Numeric network IDs come from the matching ItemRegistry packet;
/// omitted entries remain unsupported gaps and are never renumbered.
#[must_use]
pub fn vanilla_item_registry() -> Arc<[ItemRegistryEntry]> {
    const RETAIL_ITEMS: &str = include_str!("../../data/retail_items_1_26_60.tsv");

    let mut entries = Vec::with_capacity(RETAIL_ITEMS.lines().count());
    for line in RETAIL_ITEMS.lines() {
        let (network_id, identifier) = line
            .split_once('\t')
            .expect("retail item row must contain an ID and identifier");
        let network_id = network_id
            .parse::<i32>()
            .expect("retail item ID must be an i32");
        entries.push(ItemRegistryEntry {
            identifier: Arc::from(identifier),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        });
    }
    Arc::from(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRegistryEvent {
    pub entries: Arc<[ItemRegistryEntry]>,
}
