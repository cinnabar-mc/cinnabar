//! Matching and atomic requests for repairing an item by combining two copies in the grid.
use super::*;
use protocol::{NetworkItemStack, RecipeCatalog};
use sha2::{Digest, Sha256};

/// What the combined copy gains over the two remaining durabilities, in percent of the maximum.
const REPAIR_BONUS_PERCENT: u64 = 5;

/// The damage two copies of an item merge into: their remaining durability plus the repair
/// bonus, never more than the item's maximum.
#[must_use]
pub fn merged_damage(max: u32, first: u32, second: u32) -> u32 {
    let remaining = |damage: u32| u64::from(max.saturating_sub(damage));
    let total = remaining(first) + remaining(second) + u64::from(max) * REPAIR_BONUS_PERCENT / 100;
    max - u32::try_from(total.min(u64::from(max))).unwrap_or(max)
}

/// The repaired item and the two grid cells its special recipe consumes.
#[derive(Clone, Debug)]
pub struct RepairCraft {
    recipe_id: u32,
    output: NetworkItemStack,
    sources: [(u8, i32); 2],
}

impl RepairCraft {
    /// Returns the repaired display stack without assigning a speculative request identity.
    pub fn output(&self) -> &NetworkItemStack {
        &self.output
    }
}

impl PlayerInventoryLedger {
    /// Matches exactly two identical, damaged, plain items in the grid and previews the
    /// repaired copy, when the server advertises the repair recipe.
    pub(crate) fn repair_craft(&self, catalog: &RecipeCatalog) -> Option<RepairCraft> {
        let recipe_id = catalog.repair_multi_recipe_id()?;
        let mut found = Vec::with_capacity(2);
        for slot in self.crafting_grid().slots() {
            let Some(stack) = self.target_stack(super::super::InventoryTarget::Craft(slot)) else {
                continue;
            };
            if found.len() == 2 || !recipe_input_valid(stack) {
                return None;
            }
            found.push((slot, stack));
        }
        let [(first_slot, first), (second_slot, second)] = found[..] else {
            return None;
        };
        if first.count != 1
            || second.count != 1
            || first.network_id != second.network_id
            || first.metadata != second.metadata
        {
            return None;
        }
        let max = self
            .item_facts()
            .get(first.network_id)
            .durability
            .maximum()?;
        let damages = [first, second].map(|stack| protocol::item_stack_damage(stack).unwrap_or(0));
        if damages == [0, 0] || !plain_item(first) || !plain_item(second) {
            return None;
        }
        let extra = protocol::edit_item_extra(
            &first.extra_data,
            &protocol::ItemNbtEdit {
                damage: Some(i32::try_from(merged_damage(max, damages[0], damages[1])).ok()?),
                ..Default::default()
            },
        )?;
        let mut output = first.clone();
        output.stack_network_id = 0;
        output.nbt_digest = Sha256::digest(&extra).into();
        output.extra_data = extra;
        Some(RepairCraft {
            recipe_id,
            output,
            sources: [
                (first_slot, first.stack_network_id),
                (second_slot, second.stack_network_id),
            ],
        })
    }

    /// Revalidates the result and consumes both copies through shared craft placement.
    pub(crate) fn begin_repair_craft(
        &mut self,
        recipe: &RepairCraft,
        sink: CraftSink,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_special_craft(recipe.recipe_id, &recipe.output, &recipe.sources, sink)
    }
}

/// Whether the stack carries nothing a repair would have to merge or drop: no enchantments, name
/// or unbreakable flag, and readable data.
fn plain_item(stack: &NetworkItemStack) -> bool {
    protocol::item_anvil_data(&stack.extra_data)
        .is_some_and(|data| data.enchantments.is_empty() && data.custom_name.is_none())
        && !protocol::item_extra_unbreakable(&stack.extra_data)
        && !protocol::item_has_trim(&stack.extra_data)
}

#[cfg(test)]
mod tests {
    use super::merged_damage;

    /// Two copies add their remaining durability plus a twentieth of the maximum.
    #[test]
    fn two_copies_add_remaining_durability_and_a_bonus() {
        // 385 - 100 + 385 - 50 + 19 = 639, clamped to the maximum.
        assert_eq!(merged_damage(385, 100, 50), 0);
        // 385 - 300 + 385 - 350 + 19 = 139 remaining.
        assert_eq!(merged_damage(385, 300, 350), 385 - 139);
    }

    /// The result never exceeds the maximum and damages beyond it count as fully worn.
    #[test]
    fn the_result_is_clamped_to_the_maximum() {
        assert_eq!(merged_damage(100, 0, 0), 0);
        assert_eq!(merged_damage(100, 1000, 1000), 100 - 5);
        assert_eq!(merged_damage(u32::MAX, 1, 1), 0);
    }
}
