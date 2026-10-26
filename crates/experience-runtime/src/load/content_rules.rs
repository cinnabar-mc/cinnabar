//! Server WIT 0.12's content rules: what each block drops by itself, how its item is offered and
//! whether it takes part in redstone, and which items the creative inventory leaves out. The
//! adapter registers what they set.

use std::collections::HashSet;

use anyhow::{Result, bail, ensure};

use crate::host::cinnabar::experience_server::types as wit;
use crate::protocol::{BlockDef, BlockDrops, BlockItem, ItemDef};

/// Applies `rules` to the Experience's validated `blocks` and `items`: each rule names one of its
/// blocks, once, and a block without an item does not drop itself; each unlisted item is one of
/// its items, named once.
pub(super) fn apply(
    mut blocks: Vec<BlockDef>,
    mut items: Vec<ItemDef>,
    rules: wit::ContentRules,
) -> Result<(Vec<BlockDef>, Vec<ItemDef>)> {
    let mut seen = HashSet::with_capacity(rules.blocks.len());
    for rule in rules.blocks {
        let id = rule.block;
        let block = blocks.iter_mut().find(|block| block.id == id);
        let Some(block) = block else {
            bail!("block rule \"{id}\" names none of the Experience's blocks");
        };
        ensure!(seen.insert(id.clone()), "block \"{id}\" has two rules");
        block.drops = match rule.drops {
            wit::BlockDrops::Itself => BlockDrops::Itself,
            wit::BlockDrops::Nothing => BlockDrops::Nothing,
        };
        block.item = match rule.item {
            wit::BlockItem::Listed => BlockItem::Listed,
            wit::BlockItem::Unlisted => BlockItem::Unlisted,
            wit::BlockItem::None => BlockItem::None,
        };
        block.redstone = rule.redstone;
        ensure!(
            !(block.item == BlockItem::None && block.drops == BlockDrops::Itself),
            "block \"{id}\" has no item, so it cannot drop itself"
        );
    }
    let mut seen = HashSet::with_capacity(rules.unlisted_items.len());
    for id in rules.unlisted_items {
        let Some(item) = items.iter_mut().find(|item| item.id == id) else {
            bail!("unlisted item \"{id}\" is none of the Experience's items");
        };
        ensure!(seen.insert(id.clone()), "item \"{id}\" is unlisted twice");
        item.listed = false;
    }
    Ok((blocks, items))
}

#[cfg(test)]
mod tests;
