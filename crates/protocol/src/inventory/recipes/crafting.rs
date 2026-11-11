//! Immutable public views of decoded crafting recipes.

use super::catalog::RecipeCatalog;
use super::model::{Ingredient, RecipeHandle};
use std::sync::Arc;

/// The output a recipe declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeOutput {
    pub network_id: i32,
    pub aux: u16,
    pub count: u8,
    pub block_runtime_id: u32,
    /// The result's complete item data, kept byte for byte.
    pub user_data: RecipeOutputData,
}

/// User data on a recipe result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeOutputData {
    /// No user-data buffer was supplied.
    Absent,
    /// An empty compound header and two empty restriction lists.
    Empty,
    /// The empty envelope followed by a shield's blocking timestamp.
    Shield { blocking_tick: i64 },
    /// Any other complete item data, such as a server's custom name, lore or enchantments.
    Tagged(Arc<[u8]>),
}

impl RecipeOutputData {
    /// Whether a result's item data is one complete, well-formed envelope, without copying it.
    pub(super) fn admits(extra: &[u8]) -> bool {
        super::canonical_empty_extra(extra) || crate::item::item_user_data_complete(extra)
    }

    /// Copies a result's admitted item data, keeping plain and shield envelopes compact.
    pub(super) fn decode(extra: &[u8]) -> Option<Self> {
        if super::canonical_empty_extra(extra) {
            return Some(if extra.is_empty() {
                Self::Absent
            } else {
                Self::Empty
            });
        }
        if let Some((envelope, tick)) = extra.split_at_checked(10)
            && super::canonical_empty_extra(envelope)
            && let Ok(tick) = tick.try_into()
        {
            return Some(Self::Shield {
                blocking_tick: i64::from_le_bytes(tick),
            });
        }
        crate::item::item_user_data_complete(extra).then(|| Self::Tagged(Arc::from(extra)))
    }

    /// Recreates exactly the item data advertised by the recipe.
    pub fn bytes(&self) -> Arc<[u8]> {
        match self {
            Self::Absent => Arc::from([]),
            Self::Empty => Arc::from([0; 10]),
            Self::Shield { blocking_tick } => {
                let mut bytes = [0; 18];
                bytes[10..].copy_from_slice(&blocking_tick.to_le_bytes());
                Arc::from(bytes)
            }
            Self::Tagged(bytes) => Arc::clone(bytes),
        }
    }
}

/// One ingredient of a crafting recipe as callers may read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeIngredientView {
    pub name: Arc<str>,
    pub tag: bool,
    pub aux: u16,
    pub count: u8,
}

impl RecipeCatalog {
    /// Every retained crafting-table recipe, any shape.
    #[must_use]
    pub fn crafting_handles(&self) -> Vec<RecipeHandle> {
        self.crafting_recipes().cloned().collect()
    }
}

impl RecipeHandle {
    /// The recipe's ingredients: shaped recipes in row-major order with `None`
    /// for empty cells, shapeless ones without gaps.
    #[must_use]
    pub fn ingredient_views(&self) -> Vec<Option<RecipeIngredientView>> {
        let recipe = self.recipe();
        let view = |ingredient: &Ingredient| RecipeIngredientView {
            name: Arc::from(ingredient.name.as_str()),
            tag: ingredient.tag,
            aux: ingredient.aux,
            count: ingredient.count,
        };
        if recipe.shapeless {
            recipe
                .ingredients
                .iter()
                .flatten()
                .map(|i| Some(view(i)))
                .collect()
        } else {
            let cells = usize::from(recipe.width) * usize::from(recipe.height);
            recipe
                .ingredients
                .iter()
                .take(cells)
                .map(|slot| slot.as_ref().map(view))
                .collect()
        }
    }

    /// Whether the recipe is a shapeless one.
    #[must_use]
    pub fn is_shapeless(&self) -> bool {
        self.recipe().shapeless
    }

    #[must_use]
    pub fn output(&self) -> RecipeOutput {
        self.recipe().output()
    }

    /// Results after [`Self::output`], such as the buckets a cake returns, in wire order.
    pub fn extra_outputs(&self) -> impl ExactSizeIterator<Item = RecipeOutput> + '_ {
        self.recipe().extra_outputs()
    }

    /// Per-craft ingredient counts in recipe order, empty shape cells skipped.
    pub fn ingredient_counts(&self) -> impl Iterator<Item = u8> + '_ {
        self.recipe()
            .ingredients
            .iter()
            .flatten()
            .map(|ingredient| ingredient.count)
    }
}
