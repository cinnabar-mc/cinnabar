//! The efficiency attribute a vanilla tool shows in its tooltip.
//!
//! Each tool family names its own attribute, and the displayed value is the destroy
//! speed of the tool's material, which mining also uses.

use crate::facts::{ItemFact, ItemTraits};

/// The attribute a tool family reports its destroy speed through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolEfficiency {
    /// Pickaxes: `attribute.name.generic.miningEfficiency`.
    Mining,
    /// Axes: `attribute.name.generic.chopEfficiency`.
    Chop,
    /// Shovels: `attribute.name.generic.digEfficiency`.
    Dig,
}

/// Returns the efficiency attribute and whole value of a tiered pickaxe, axe or shovel.
/// Swords, hoes and untiered tools show none.
#[must_use]
pub fn vanilla_tool_efficiency(fact: &ItemFact) -> Option<(ToolEfficiency, u32)> {
    let kind = if fact.traits.contains(ItemTraits::PICKAXE) {
        ToolEfficiency::Mining
    } else if fact.traits.contains(ItemTraits::AXE) {
        ToolEfficiency::Chop
    } else if fact.traits.contains(ItemTraits::SHOVEL) {
        ToolEfficiency::Dig
    } else {
        return None;
    };
    Some((kind, fact.tier?.destroy_speed()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ItemFacts, ItemTier};

    fn efficiency(identifier: &str) -> Option<(ToolEfficiency, u32)> {
        vanilla_tool_efficiency(ItemFacts::builtin().unwrap().by_identifier(identifier)?)
    }

    #[test]
    fn each_tool_family_reports_its_material_speed_through_its_attribute() {
        let tiers = [
            ("wooden", ItemTier::Wood, 2),
            ("stone", ItemTier::Stone, 4),
            ("copper", ItemTier::Copper, 5),
            ("iron", ItemTier::Iron, 6),
            ("diamond", ItemTier::Diamond, 8),
            ("netherite", ItemTier::Netherite, 9),
            ("golden", ItemTier::Gold, 12),
        ];
        for (name, tier, speed) in tiers {
            assert_eq!(tier.destroy_speed(), speed, "{name}");
            for (family, kind) in [
                ("pickaxe", ToolEfficiency::Mining),
                ("axe", ToolEfficiency::Chop),
                ("shovel", ToolEfficiency::Dig),
            ] {
                let identifier = format!("minecraft:{name}_{family}");
                assert_eq!(efficiency(&identifier), Some((kind, speed)), "{identifier}");
            }
        }
    }

    #[test]
    fn swords_hoes_and_other_items_show_no_efficiency() {
        for identifier in [
            "minecraft:diamond_sword",
            "minecraft:diamond_hoe",
            "minecraft:shears",
            "minecraft:diamond_helmet",
            "minecraft:stick",
        ] {
            assert_eq!(efficiency(identifier), None, "{identifier}");
        }
    }
}
