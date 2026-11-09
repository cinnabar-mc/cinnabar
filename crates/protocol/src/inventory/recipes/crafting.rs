//! Immutable public views of decoded crafting recipes.

use super::catalog::RecipeCatalog;
use super::model::{Ingredient, RecipeHandle};

/// The output a recipe declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeOutput {
    pub network_id: i32,
    pub aux: u16,
    pub count: u8,
    pub block_runtime_id: u32,
    /// Plain item data, including a shield's trailing blocking timestamp.
    pub user_data: RecipeOutputData,
}

/// User data on a recipe result with no saved tags or item restrictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeOutputData {
    /// No user-data buffer was supplied.
    Absent,
    /// An empty compound header and two empty restriction lists.
    Empty,
    /// The empty envelope followed by a shield's blocking timestamp.
    Shield { blocking_tick: i64 },
}

impl RecipeOutputData {
    /// Reads a plain result's complete envelope and retains its runtime-only fields.
    pub(super) fn decode(extra: &[u8]) -> Option<Self> {
        if super::canonical_empty_extra(extra) {
            return Some(if extra.is_empty() {
                Self::Absent
            } else {
                Self::Empty
            });
        }
        let (envelope, tick) = extra.split_at_checked(10)?;
        if !super::canonical_empty_extra(envelope) {
            return None;
        }
        Some(Self::Shield {
            blocking_tick: i64::from_le_bytes(tick.try_into().ok()?),
        })
    }

    /// Recreates exactly the plain item data advertised by the recipe.
    pub fn bytes(self) -> std::sync::Arc<[u8]> {
        match self {
            Self::Absent => std::sync::Arc::from([]),
            Self::Empty => std::sync::Arc::from([0; 10]),
            Self::Shield { blocking_tick } => {
                let mut bytes = [0; 18];
                bytes[10..].copy_from_slice(&blocking_tick.to_le_bytes());
                std::sync::Arc::from(bytes)
            }
        }
    }
}

/// One ingredient of a crafting recipe as callers may read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeIngredientView {
    pub name: std::sync::Arc<str>,
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
            name: std::sync::Arc::from(ingredient.name.as_str()),
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
