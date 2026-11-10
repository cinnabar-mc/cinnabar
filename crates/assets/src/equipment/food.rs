//! Installation-owned food facts carried beside equipment timing metadata.
use crate::facts::{ItemFact, ItemFacts};
use crate::item_use::ItemUseProfile;
use crate::{AssetError, ItemDisplayScalar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Exact nutrition and saturation component values for one item identifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledItemFood {
    pub identifier: Box<str>,
    pub nutrition: u32,
    pub saturation_modifier: ItemDisplayScalar,
}

impl super::RuntimeEquipmentCatalog {
    /// Finds installed food for an item identifier, falling back to the pinned legacy name its
    /// use profile aliases. Unknown identifiers have no food rather than an estimate.
    pub fn installed_item_food(&self, identifier: &str) -> Option<&CompiledItemFood> {
        self.item_food(identifier).or_else(|| {
            let facts = ItemFacts::builtin().ok()?;
            self.item_food(legacy_name(facts, facts.by_identifier(identifier)?)?)
        })
    }

    /// The item identifiers [`Self::installed_item_food`] resolves, sorted, with their food:
    /// pinned items with their own or their legacy name's definition, and definitions under names
    /// no pinned item has or aliases. Legacy names are listed only as the items aliasing them.
    pub fn installed_item_foods(&self) -> Vec<(&str, &CompiledItemFood)> {
        let Ok(facts) = ItemFacts::builtin() else {
            return self.item_food.iter().map(by_own_name).collect();
        };
        let mut aliased = BTreeSet::new();
        let mut foods: Vec<_> = facts
            .payload()
            .records
            .iter()
            .filter_map(|record| {
                let legacy = legacy_name(facts, &record.fact);
                aliased.extend(legacy);
                let food = self
                    .item_food(&record.identifier)
                    .or_else(|| self.item_food(legacy?))?;
                Some((record.identifier.as_ref(), food))
            })
            .collect();
        foods.extend(
            self.item_food
                .iter()
                .filter(|food| {
                    facts.by_identifier(&food.identifier).is_none()
                        && !aliased.contains(food.identifier.as_ref())
                })
                .map(by_own_name),
        );
        foods.sort_unstable_by_key(|(identifier, _)| *identifier);
        foods
    }
}

/// A definition keyed by the name it was authored under.
fn by_own_name(food: &CompiledItemFood) -> (&str, &CompiledItemFood) {
    (&food.identifier, food)
}

/// The legacy name a pinned item's use profile aliases, which an installation may define its
/// food under.
fn legacy_name<'a>(facts: &'a ItemFacts, fact: &ItemFact) -> Option<&'a str> {
    match facts.use_profile(fact.use_profile?)? {
        ItemUseProfile::Provisional { duration_alias, .. } => duration_alias.as_deref(),
        ItemUseProfile::Unresolved => None,
    }
}

/// Validates sorted bounded identifiers and finite nonnegative authored food values.
pub(super) fn validate(entries: &[CompiledItemFood]) -> Result<(), AssetError> {
    if entries.len() > super::MAX_ITEM_TIMINGS {
        return Err(super::invalid("too many food definitions"));
    }
    let mut previous: Option<&str> = None;
    for entry in entries {
        let id = entry.identifier.as_ref();
        if id.is_empty()
            || id.len() > super::MAX_EQUIPMENT_IDENTIFIER_BYTES
            || !id.contains(':')
            || previous.is_some_and(|value| value >= id)
            || entry.saturation_modifier.get() < 0.0
        {
            return Err(super::invalid("invalid or unordered food definition"));
        }
        previous = Some(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RuntimeEquipmentCatalog, encode_equipment_catalog_full,
        encode_equipment_catalog_with_use_profiles,
    };
    /// Listed foods are exactly the identifiers the lookup resolves: a legacy name appears as
    /// the pinned item aliasing it, and a name no pinned item has or aliases stays listed.
    #[test]
    fn installed_foods_list_the_identifiers_the_lookup_resolves() {
        let entry = |identifier: &str, nutrition| CompiledItemFood {
            identifier: identifier.into(),
            nutrition,
            saturation_modifier: ItemDisplayScalar::new(0.5).unwrap(),
        };
        let bytes = encode_equipment_catalog_with_use_profiles(
            [1; 32],
            [2; 32],
            &[],
            &[],
            crate::equipment::EquipmentItemFacts {
                item_food: &[
                    entry("fixture:snack", 1),
                    entry("minecraft:apple", 4),
                    entry("minecraft:muttonCooked", 6),
                ],
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        let foods = catalog.installed_item_foods();
        let listed: Vec<_> = foods
            .iter()
            .map(|(identifier, food)| (*identifier, food.nutrition))
            .collect();
        assert_eq!(
            listed,
            [
                ("fixture:snack", 1),
                ("minecraft:apple", 4),
                ("minecraft:cooked_mutton", 6)
            ]
        );
        for (identifier, food) in foods {
            assert_eq!(catalog.installed_item_food(identifier), Some(food));
        }
    }

    #[test]
    fn optional_food_roundtrips_and_old_carriers_stay_empty() {
        let old = encode_equipment_catalog_full([1; 32], [2; 32], &[], &[], &[]).unwrap();
        assert!(
            RuntimeEquipmentCatalog::decode(&old)
                .unwrap()
                .item_foods()
                .is_empty()
        );
        let food = CompiledItemFood {
            identifier: "fixture:food".into(),
            nutrition: 4,
            saturation_modifier: ItemDisplayScalar::new(0.3).unwrap(),
        };
        let overlay = crate::equipment::CompiledItemUseOverlay {
            identifier: food.identifier.clone(),
            duration_ticks: Some(20),
            food_can_always_eat: Some(false),
            use_modifiers_present: true,
            ..Default::default()
        };
        let bytes = encode_equipment_catalog_with_use_profiles(
            [1; 32],
            [2; 32],
            &[],
            &[],
            crate::equipment::EquipmentItemFacts {
                item_use_overlays: std::slice::from_ref(&overlay),
                item_food: std::slice::from_ref(&food),
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.item_food("fixture:food"), Some(&food));
        assert_eq!(catalog.item_use_overlays(), &[overlay]);
        assert!(catalog.item_food("fixture:unknown").is_none());
        let mut bad = food.clone();
        bad.saturation_modifier = ItemDisplayScalar::new(-0.3).unwrap();
        assert!(
            encode_equipment_catalog_with_use_profiles(
                [1; 32],
                [2; 32],
                &[],
                &[],
                crate::equipment::EquipmentItemFacts {
                    item_food: &[bad],
                    ..Default::default()
                },
            )
            .is_err()
        );
        assert!(
            encode_equipment_catalog_with_use_profiles(
                [1; 32],
                [2; 32],
                &[],
                &[],
                crate::equipment::EquipmentItemFacts {
                    item_food: &[food.clone(), food],
                    ..Default::default()
                },
            )
            .is_err()
        );
    }
}
