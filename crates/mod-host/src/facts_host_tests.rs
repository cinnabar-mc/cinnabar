use super::*;
use crate::ModGrants;
use wit::Host as _;

/// A host state holding the player-state grant.
fn state() -> State {
    State::new(
        ModGrants {
            player_state: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    )
}

/// One installed food definition.
fn entry(identifier: &str, nutrition: u32, saturation: f32) -> assets::CompiledItemFood {
    assets::CompiledItemFood {
        identifier: identifier.into(),
        nutrition,
        saturation_modifier: assets::ItemDisplayScalar::new(saturation).unwrap(),
    }
}

/// An installed food table of `entries`, sorted by identifier.
fn table(entries: &[assets::CompiledItemFood]) -> Arc<assets::RuntimeEquipmentCatalog> {
    let bytes = assets::encode_equipment_catalog_with_use_profiles(
        [1; 32],
        [2; 32],
        &[],
        &[],
        assets::EquipmentItemFacts {
            item_food: entries,
            ..Default::default()
        },
    )
    .unwrap();
    Arc::new(assets::RuntimeEquipmentCatalog::decode(&bytes).unwrap())
}

/// An installed food table with one exact entry and one legacy-named entry.
fn food() -> Arc<assets::RuntimeEquipmentCatalog> {
    table(&[
        entry("minecraft:apple", 4, 0.3),
        entry("minecraft:muttonCooked", 6, 0.8),
    ])
}

/// Durability comes from the pinned item facts; unknown identifiers have no facts.
#[test]
fn item_facts_report_pinned_durability() {
    let mut state = state();
    let helmet = state
        .read_item("minecraft:diamond_helmet".into(), 0)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(helmet.max_durability, Some(363));
    assert!(helmet.food.is_none() && helmet.potion_effects.is_empty());
    let dirt = state
        .read_item("minecraft:dirt".into(), 0)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(dirt.max_durability, None);
    assert!(
        state
            .read_item("fixture:unknown".into(), 0)
            .unwrap()
            .unwrap()
            .is_none()
    );
}

/// Food comes from the installed table, including pinned legacy names, and is absent without it.
#[test]
fn item_facts_report_installed_food() {
    let mut state = state();
    let apple = |state: &mut State| {
        state
            .read_item("minecraft:apple".into(), 0)
            .unwrap()
            .unwrap()
            .unwrap()
            .food
    };
    assert!(apple(&mut state).is_none());
    state.facts.set_food(Some(food()));
    let food = apple(&mut state).unwrap();
    assert_eq!(food.nutrition, 4);
    assert!((food.saturation_modifier - 0.3).abs() < 1e-6);
    let mutton = state
        .read_item("minecraft:cooked_mutton".into(), 0)
        .unwrap()
        .unwrap()
        .unwrap()
        .food
        .unwrap();
    assert_eq!(mutton.nutrition, 6);
}

/// Listed foods use the identifiers `read-item` resolves and agree with it; without a table the
/// list is empty, and a new table replaces it.
#[test]
fn foods_list_installed_food_under_resolvable_identifiers() {
    let mut state = state();
    assert!(state.read_foods().unwrap().unwrap().is_empty());
    state.facts.set_food(Some(food()));
    let foods = state.read_foods().unwrap().unwrap();
    let identifiers: Vec<_> = foods.iter().map(|item| item.identifier.as_str()).collect();
    assert_eq!(identifiers, ["minecraft:apple", "minecraft:cooked_mutton"]);
    for item in &foods {
        assert_eq!(item.metadata, None);
        let read = state
            .read_item(item.identifier.clone(), 0)
            .unwrap()
            .unwrap()
            .unwrap()
            .food
            .unwrap();
        assert_eq!(read.nutrition, item.food.nutrition);
        assert_eq!(read.saturation_modifier, item.food.saturation_modifier);
    }
    state.facts.set_food(None);
    assert!(state.read_foods().unwrap().unwrap().is_empty());
}

/// The list keeps the first `MAX_FOOD_ITEMS` foods by identifier and leaves out identifiers
/// longer than `read-item` accepts.
#[test]
fn foods_are_bounded_for_the_guest() {
    let long = format!("fixture:{}", "a".repeat(mod_api::MAX_ITEM_IDENTIFIER_BYTES));
    let mut entries = vec![entry(&long, 1, 0.5)];
    entries.extend(
        (0..=mod_api::facts::MAX_FOOD_ITEMS)
            .map(|index| entry(&format!("fixture:food_{index:04}"), 1, 0.5)),
    );
    let mut state = state();
    state.facts.set_food(Some(table(&entries)));
    let foods = state.read_foods().unwrap().unwrap();
    assert_eq!(foods.len(), mod_api::facts::MAX_FOOD_ITEMS);
    assert_eq!(foods[0].identifier, "fixture:food_0000");
    assert_eq!(
        foods[mod_api::facts::MAX_FOOD_ITEMS - 1].identifier,
        format!("fixture:food_{:04}", mod_api::facts::MAX_FOOD_ITEMS - 1)
    );
}

/// Potions, splash and lingering potions and tipped arrows report their potion's effects.
#[test]
fn item_facts_report_potion_effects_by_metadata() {
    let mut state = state();
    let mut effects = |identifier: &str, metadata| {
        state
            .read_item(identifier.into(), metadata)
            .unwrap()
            .unwrap()
            .unwrap()
            .potion_effects
    };
    let speed = assets::effect_descriptor_by_identifier("minecraft:speed")
        .unwrap()
        .id;
    let swiftness = |amplifier, duration_ticks| {
        vec![wit::PotionEffect {
            effect_id: speed,
            amplifier,
            duration_ticks: Some(duration_ticks),
        }]
    };
    assert_eq!(effects("minecraft:potion", 14), swiftness(0, 3600));
    assert_eq!(effects("minecraft:splash_potion", 16), swiftness(1, 1800));
    assert_eq!(effects("minecraft:lingering_potion", 14), swiftness(0, 900));
    assert_eq!(effects("minecraft:arrow", 15), swiftness(0, 3600));
    assert!(effects("minecraft:arrow", 0).is_empty());
    assert!(effects("minecraft:potion", 0).is_empty());
    assert!(effects("minecraft:potion", u32::MAX).is_empty());
    assert!(effects("minecraft:apple", 14).is_empty());
}

/// Effects report their name key and extensionless icon; unknown effects have no facts.
#[test]
fn effect_facts_report_name_and_icon() {
    let mut state = state();
    let speed = state.read_effect(1).unwrap().unwrap().unwrap();
    assert_eq!(speed.name_key, "potion.moveSpeed");
    assert_eq!(speed.texture, "textures/ui/speed_effect");
    let instant = state.read_effect(6).unwrap().unwrap().unwrap();
    assert!(instant.texture.is_empty());
    assert!(state.read_effect(0).unwrap().unwrap().is_none());
    assert!(state.read_effect(1000).unwrap().unwrap().is_none());
}

/// Facts require player-state authority and have their own per-callback read budget.
#[test]
fn facts_are_granted_and_bounded_separately() {
    let mut denied = State::new(ModGrants::default(), String::new(), Default::default());
    assert!(denied.read_effect(1).unwrap().is_err());
    assert!(denied.read_foods().unwrap().is_err());
    assert!(
        denied
            .read_item("minecraft:apple".into(), 0)
            .unwrap()
            .is_err()
    );
    let mut state = state();
    assert!(
        state
            .read_item("x".repeat(mod_api::MAX_ITEM_IDENTIFIER_BYTES + 1), 0)
            .unwrap()
            .is_err()
    );
    state.facts.begin();
    for index in 0..mod_api::MAX_FACT_READS {
        if index % 2 == 0 {
            state.read_effect(1).unwrap().unwrap();
        } else {
            state.read_foods().unwrap().unwrap();
        }
    }
    assert!(state.read_foods().is_err(), "one more read traps");
    use crate::runtime::cinnabar::extension::player_state::Host as _;
    assert!(
        state.read_fps().unwrap().is_ok(),
        "player-state reads keep their own budget"
    );
    state.facts.begin();
    assert!(state.read_effect(1).unwrap().is_ok());
}
