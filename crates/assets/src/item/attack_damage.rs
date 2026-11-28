//! The attack damage a vanilla melee item adds to the player's base attack.
//!
//! Bedrock's tooltip shows the item's own `minecraft:damage`, which excludes the player's
//! base attack of 1. Vanilla items define no damage component on the wire, so the value
//! follows the item's family and material facts instead of a per-identifier list.

use crate::facts::{ItemFact, ItemTier, ItemTraits};

/// Damage added by a wooden item of each family; later tiers add [`tier_bonus`].
const FAMILY_BASE: [(ItemTraits, u32); 6] = [
    (ItemTraits::SWORD, 4),
    (ItemTraits::AXE, 3),
    (ItemTraits::SPEAR, 1),
    (ItemTraits::PICKAXE, 2),
    (ItemTraits::SHOVEL, 1),
    (ItemTraits::HOE, 2),
];
/// A trident's fixed melee damage.
const TRIDENT_DAMAGE: u32 = 8;
/// A mace's fixed melee damage.
const MACE_DAMAGE: u32 = 5;

/// Damage a tier adds over wood within one family.
const fn tier_bonus(tier: ItemTier) -> u32 {
    match tier {
        ItemTier::Wood | ItemTier::Gold => 0,
        ItemTier::Stone | ItemTier::Copper => 1,
        ItemTier::Iron => 2,
        ItemTier::Diamond => 3,
        ItemTier::Netherite => 4,
    }
}

/// Returns the damage a vanilla melee item adds to the player's base attack, or `None`
/// for items that are not melee weapons or tools of a known material.
#[must_use]
pub fn vanilla_attack_damage(fact: &ItemFact) -> Option<u32> {
    if fact.traits.contains(ItemTraits::TRIDENT) {
        return Some(TRIDENT_DAMAGE);
    }
    if fact.traits.contains(ItemTraits::MACE) {
        return Some(MACE_DAMAGE);
    }
    let (_, base) = FAMILY_BASE
        .iter()
        .find(|(family, _)| fact.traits.contains(*family))?;
    Some(base + tier_bonus(fact.tier?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::ItemFacts;

    fn damage(identifier: &str) -> Option<u32> {
        vanilla_attack_damage(ItemFacts::builtin().unwrap().by_identifier(identifier)?)
    }

    /// Bedrock's per-material melee table, minus the player's base attack of 1.
    #[test]
    fn tools_and_weapons_add_their_bedrock_damage_by_material() {
        let tiers = [
            ("wooden", [4, 3, 1, 2, 1, 2]),
            ("golden", [4, 3, 1, 2, 1, 2]),
            ("stone", [5, 4, 2, 3, 2, 3]),
            ("copper", [5, 4, 2, 3, 2, 3]),
            ("iron", [6, 5, 3, 4, 3, 4]),
            ("diamond", [7, 6, 4, 5, 4, 5]),
            ("netherite", [8, 7, 5, 6, 5, 6]),
        ];
        let families = ["sword", "axe", "spear", "pickaxe", "shovel", "hoe"];
        for (tier, expected) in tiers {
            for (family, expected) in families.iter().zip(expected) {
                let identifier = format!("minecraft:{tier}_{family}");
                assert_eq!(damage(&identifier), Some(expected), "{identifier}");
            }
        }
        assert_eq!(damage("minecraft:trident"), Some(8));
        assert_eq!(damage("minecraft:mace"), Some(5));
    }

    #[test]
    fn other_items_add_no_attack_damage() {
        for identifier in [
            "minecraft:stick",
            "minecraft:bow",
            "minecraft:shears",
            "minecraft:iron_helmet",
            "minecraft:fishing_rod",
        ] {
            assert_eq!(damage(identifier), None, "{identifier}");
        }
    }
}
