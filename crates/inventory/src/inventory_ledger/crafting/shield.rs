//! Matching and atomic requests for shield decoration.
use super::*;
use protocol::{NetworkItemStack, RecipeCatalog};
use sha2::{Digest, Sha256};

/// The concrete shield result and the current cells consumed by its special recipe.
#[derive(Clone, Debug)]
pub struct ShieldCraft {
    recipe_id: u32,
    output: NetworkItemStack,
    sources: [(u8, i32); 2],
}

impl ShieldCraft {
    /// Returns the decorated display stack without assigning a speculative request identity.
    pub fn output(&self) -> &NetworkItemStack {
        &self.output
    }
}

impl PlayerInventoryLedger {
    /// Matches exactly one shield and banner on the workbench using current stack metadata.
    pub(crate) fn shield_craft(&self, catalog: &RecipeCatalog) -> Option<ShieldCraft> {
        if self.crafting_grid() != CraftingGrid::Workbench {
            return None;
        }
        let recipe_id = catalog.shield_multi_recipe_id()?;
        let (mut shield, mut banner) = (None, None);
        for slot in self.crafting_grid().slots() {
            let Some(stack) = self.target_stack(super::super::InventoryTarget::Craft(slot)) else {
                continue;
            };
            if !recipe_input_valid(stack) {
                return None;
            }
            let identifier = self
                .negotiated_item_entry(stack.network_id)?
                .identifier
                .as_ref();
            let target = if identifier == assets::facts::VanillaItem::Shield.identifier() {
                &mut shield
            } else if identifier == assets::facts::BANNER_IDENTIFIER {
                &mut banner
            } else {
                return None;
            };
            if target.replace((slot, stack)).is_some() {
                return None;
            }
        }
        let ((shield_slot, shield), (banner_slot, banner)) = (shield?, banner?);
        let extra = protocol::apply_banner_to_shield(
            &shield.extra_data,
            &banner.extra_data,
            banner.metadata,
        )?;
        let mut output = shield.clone();
        output.count = 1;
        output.stack_network_id = 0;
        output.nbt_digest = Sha256::digest(&extra).into();
        output.extra_data = extra;
        let mut sources = [
            (shield_slot, shield.stack_network_id),
            (banner_slot, banner.stack_network_id),
        ];
        sources.sort_unstable_by_key(|source| source.0);
        Some(ShieldCraft {
            recipe_id,
            output,
            sources,
        })
    }

    /// Revalidates the result and consumes one of each ingredient through shared craft placement.
    pub(crate) fn begin_shield_craft(
        &mut self,
        recipe: &ShieldCraft,
        sink: CraftSink,
    ) -> Result<i32, InventoryGestureError> {
        self.gesture_preflight(true)?;
        let mut consumed = Vec::new();
        for &(slot, id) in &recipe.sources {
            let cell = Cell::Craft(slot);
            self.check_surfaces([cell])?;
            let held = self
                .view()
                .get(cell)
                .ok_or(InventoryGestureError::InvalidRequest)?;
            if held.stack.stack_network_id != id || held.stack.count == 0 {
                return Err(InventoryGestureError::InvalidRequest);
            }
            if self.awaiting_identity(held) {
                return Err(InventoryGestureError::AwaitingIdentity);
            }
            consumed.push((cell, 1, id));
        }
        let request_id = self.peek_request_id()?;
        let mut created = recipe.output.clone();
        created.stack_network_id = request_id;
        let entry = self
            .negotiated_item_entry(created.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let result = CraftResult {
            identifier: Arc::clone(&entry.identifier),
            aux: created.metadata as i32,
            count: 1,
            block_runtime_id: u32::from_ne_bytes(created.block_runtime_id.to_ne_bytes()),
            user_data: Arc::clone(&created.extra_data),
        };
        let submission = self.finish_craft_submission(
            output::CraftSubmissionPlan {
                id: recipe.recipe_id,
                crafts: 1,
                created,
                results: Arc::from([result]),
                consumed,
                extra: None,
            },
            sink,
        )?;
        self.submit(submission)
    }
}
