//! The `facts` imports: pinned item and effect tables and the installation's food, read under
//! their own per-callback allowance.

use super::{State, cinnabar::extension::facts as wit};
use anyhow::{Result, bail};
use std::sync::Arc;

/// The installed food table, its foods once listed, and this callback's read count.
#[derive(Default)]
pub(super) struct FactsState {
    reads: u32,
    food: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    /// The `read-foods` list of `food`, built by the first read after the table changes.
    foods: Option<Arc<[wit::FoodItem]>>,
}

impl FactsState {
    /// Starts a callback with a fresh read allowance.
    pub fn begin(&mut self) {
        self.reads = 0;
    }

    /// Charges one read; exceeding [`mod_api::MAX_FACT_READS`] traps the callback.
    fn charge(&mut self) -> Result<()> {
        self.reads += 1;
        if self.reads > mod_api::MAX_FACT_READS {
            bail!("facts read budget exhausted");
        }
        Ok(())
    }

    /// Replaces the installation's food table; none leaves every item without food facts.
    pub fn set_food(&mut self, food: Option<Arc<assets::RuntimeEquipmentCatalog>>) {
        self.food = food;
        self.foods = None;
    }

    /// The installation's food table, which a reloaded instance inherits.
    pub fn food(&self) -> Option<Arc<assets::RuntimeEquipmentCatalog>> {
        self.food.clone()
    }

    /// The installed foods `read-foods` reports, listed once per food table.
    fn foods(&mut self) -> Arc<[wit::FoodItem]> {
        let food = self.food.as_deref();
        Arc::clone(self.foods.get_or_insert_with(|| food_items(food)))
    }
}

/// Converts one installed food definition.
fn wit_food(food: &assets::CompiledItemFood) -> wit::Food {
    wit::Food {
        nutrition: food.nutrition,
        saturation_modifier: food.saturation_modifier.get(),
    }
}

/// Every installed food by the identifiers `read-item` resolves, sorted, without identifiers
/// over [`mod_api::MAX_ITEM_IDENTIFIER_BYTES`] and keeping the first [`mod_api::facts::MAX_FOOD_ITEMS`].
fn food_items(catalog: Option<&assets::RuntimeEquipmentCatalog>) -> Arc<[wit::FoodItem]> {
    catalog
        .map(assets::RuntimeEquipmentCatalog::installed_item_foods)
        .unwrap_or_default()
        .into_iter()
        .filter(|(identifier, _)| identifier.len() <= mod_api::MAX_ITEM_IDENTIFIER_BYTES)
        .take(mod_api::facts::MAX_FOOD_ITEMS)
        .map(|(identifier, definition)| wit::FoodItem {
            identifier: identifier.into(),
            metadata: None,
            food: wit_food(definition),
        })
        .collect()
}

/// Resolves an identifier through the pinned item facts and installed food; none when neither
/// knows it.
fn item_facts(
    identifier: &str,
    metadata: u32,
    food: Option<&assets::RuntimeEquipmentCatalog>,
) -> Option<wit::ItemFacts> {
    let fact = assets::facts::ItemFacts::builtin()
        .ok()
        .and_then(|facts| facts.by_identifier(identifier));
    let food = food
        .and_then(|catalog| catalog.installed_item_food(identifier))
        .map(wit_food);
    if fact.is_none() && food.is_none() {
        return None;
    }
    Some(wit::ItemFacts {
        max_durability: fact.and_then(|fact| fact.durability.maximum()),
        food,
        potion_effects: potion_effects(identifier, fact, metadata),
    })
}

/// The effects of the potion an item carries: potions, splash and lingering potions by their
/// metadata, with a lingering potion's quarter durations, and tipped arrows by metadata minus
/// one, with the carried potion's own durations.
fn potion_effects(
    identifier: &str,
    fact: Option<&assets::facts::ItemFact>,
    metadata: u32,
) -> Vec<wit::PotionEffect> {
    let (potion, divisor) = if let Some(form) = fact.and_then(|fact| fact.potion_form) {
        (i16::try_from(metadata).ok(), form.duration_divisor())
    } else if identifier == assets::facts::ARROW_IDENTIFIER {
        (
            metadata
                .checked_sub(1)
                .and_then(|potion| i16::try_from(potion).ok()),
            1,
        )
    } else {
        return Vec::new();
    };
    potion
        .and_then(assets::potion::potion_descriptor)
        .map_or_else(Vec::new, |potion| {
            potion
                .effects
                .iter()
                .filter_map(|effect| {
                    Some(wit::PotionEffect {
                        effect_id: assets::effect_descriptor_by_identifier(effect.effect)?.id,
                        amplifier: i32::from(effect.amplifier),
                        duration_ticks: effect
                            .duration_ticks
                            .map(|ticks| u64::from(ticks / divisor)),
                    })
                })
                .collect()
        })
}

/// The effect's name key and its HUD icon as JSON-UI names textures: the pack path without
/// extension, empty for effects without an icon.
fn effect_facts(effect_id: i32) -> Option<wit::EffectFacts> {
    let effect = assets::effect_descriptor(effect_id)?;
    Some(wit::EffectFacts {
        name_key: effect.name_key.into(),
        texture: effect
            .icon_path
            .map(|path| path.strip_suffix(".png").unwrap_or(path))
            .unwrap_or_default()
            .into(),
    })
}

impl wit::Host for State {
    fn read_item(
        &mut self,
        identifier: String,
        metadata: u32,
    ) -> Result<Result<Option<wit::ItemFacts>, String>> {
        self.facts.charge()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        if identifier.len() > mod_api::MAX_ITEM_IDENTIFIER_BYTES {
            return Ok(Err("item identifier exceeds its bound".into()));
        }
        Ok(Ok(item_facts(
            &identifier,
            metadata,
            self.facts.food.as_deref(),
        )))
    }

    fn read_foods(&mut self) -> Result<Result<Vec<wit::FoodItem>, String>> {
        self.facts.charge()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(self.facts.foods().to_vec()))
    }

    fn read_effect(&mut self, effect_id: i32) -> Result<Result<Option<wit::EffectFacts>, String>> {
        self.facts.charge()?;
        if !self.grants.player_state {
            return Ok(Err("player-state capability denied".into()));
        }
        Ok(Ok(effect_facts(effect_id)))
    }
}

#[cfg(test)]
#[path = "facts_host_tests.rs"]
mod tests;
