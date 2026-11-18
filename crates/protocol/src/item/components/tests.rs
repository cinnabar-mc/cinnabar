mod creative_category;
use super::parse_components;

#[test]
fn declared_use_cooldown_matches_authored_component_precision() {
    let nbt = compound(
        "",
        &[compound(
            "components",
            &[compound(
                "minecraft:cooldown",
                &[
                    string("category", "fixture:shared"),
                    float("duration", 0.549),
                ],
            )],
        )],
    );
    let cooldown = parse_components(&nbt).unwrap().use_cooldown.unwrap();
    assert_eq!(cooldown.category.as_ref(), "fixture:shared");
    assert_eq!(cooldown.ticks, 10);
}

#[test]
fn use_modifier_duration_discards_fractional_ticks() {
    for (seconds, expected) in [
        (0.049, Some(0)),
        (1.049, Some(20)),
        (1.6, Some(32)),
        (f32::NAN, None),
        (f32::INFINITY, None),
        (-1.0, None),
        (f32::MAX, None),
    ] {
        let nbt = compound(
            "",
            &[compound(
                "components",
                &[compound(
                    "minecraft:use_modifiers",
                    &[float("use_duration", seconds)],
                )],
            )],
        );
        assert_eq!(
            parse_components(&nbt).unwrap().use_duration_ticks,
            expected,
            "{seconds}"
        );
    }
}

#[test]
fn use_modifiers_presence_preserves_absent_and_invalid_movement_values() {
    for (declaration, present, movement) in [
        (None, false, None),
        (Some(compound("minecraft:use_modifiers", &[])), true, None),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[float("use_duration", 1.6)],
            )),
            true,
            None,
        ),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[float("movement_modifier", 0.0)],
            )),
            true,
            Some(0.0_f64),
        ),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[float("movement_modifier", 0.75)],
            )),
            true,
            Some(0.75_f64),
        ),
        (
            Some(string("minecraft:use_modifiers", "invalid")),
            false,
            None,
        ),
        (Some(int("minecraft:use_modifiers", 1)), false, None),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[string("movement_modifier", "0.75")],
            )),
            true,
            None,
        ),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[float("movement_modifier", -1.0)],
            )),
            true,
            None,
        ),
        (
            Some(compound(
                "minecraft:use_modifiers",
                &[float("movement_modifier", f32::NAN)],
            )),
            true,
            None,
        ),
    ] {
        let declarations: Vec<_> = declaration.into_iter().collect();
        let nbt = compound("", &[compound("components", &declarations)]);
        let facts = parse_components(&nbt).unwrap();
        assert_eq!(facts.use_modifiers_present, present);
        assert_eq!(facts.use_movement_modifier_bits, movement.map(f64::to_bits));
    }
}

fn name(tag: u8, name: &str) -> Vec<u8> {
    let mut bytes = vec![tag, name.len() as u8];
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

fn string(key: &str, value: &str) -> Vec<u8> {
    let mut bytes = name(8, key);
    bytes.push(value.len() as u8);
    bytes.extend_from_slice(value.as_bytes());
    bytes
}

fn int(key: &str, value: i32) -> Vec<u8> {
    let mut bytes = name(3, key);
    let mut zigzag = ((value << 1) ^ (value >> 31)) as u32;
    loop {
        let byte = (zigzag & 0x7f) as u8;
        zigzag >>= 7;
        if zigzag == 0 {
            bytes.push(byte);
            break;
        }
        bytes.push(byte | 0x80);
    }
    bytes
}

fn byte(key: &str, value: u8) -> Vec<u8> {
    let mut bytes = name(1, key);
    bytes.push(value);
    bytes
}

/// Encodes one finite or malformed float component for admission tests.
fn float(key: &str, value: f32) -> Vec<u8> {
    let mut bytes = name(5, key);
    bytes.extend(value.to_le_bytes());
    bytes
}
#[test]
fn explicit_food_values_are_retained_without_guessing_missing_or_nonfinite_values() {
    let food = |modifier| {
        compound(
            "",
            &[compound(
                "components",
                &[compound(
                    "minecraft:food",
                    &[int("nutrition", 4), float("saturation_modifier", modifier)],
                )],
            )],
        )
    };
    let components = parse_components(&food(0.6)).unwrap();
    let values = components.food_properties.unwrap();
    assert_eq!(values.nutrition(), 4);
    assert_eq!(values.saturation_modifier(), 0.6);
    assert!(
        parse_components(&food(f32::NAN))
            .unwrap()
            .food_properties
            .is_none()
    );
}

fn compound(key: &str, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = name(10, key);
    entries.iter().for_each(|entry| bytes.extend(entry));
    bytes.push(0);
    bytes
}

#[test]
fn custom_content_legacy_components_retain_tool_glint_and_use_facts() {
    let root = compound(
        "",
        &[compound(
            "components",
            &[
                int("minecraft:max_stack_size", 1),
                int("minecraft:max_damage", 1562),
                byte("minecraft:foil", 1),
                byte("minecraft:hand_equipped", 1),
                int("minecraft:use_duration", 32),
            ],
        )],
    );
    let facts = parse_components(&root).unwrap();
    assert_eq!(facts.max_stack_size, Some(1));
    assert_eq!(facts.max_durability, Some(1562));
    assert!(facts.durability_declared && facts.max_stack_size_component_declared);
    assert_eq!(facts.glint, Some(true));
    assert_eq!(facts.hand_equipped, Some(true));
    assert_eq!(facts.use_duration_ticks, Some(32));
}

#[test]
fn durability_declaration_retains_absent_invalid_and_nonnegative_int_maxima() {
    let scalar = |tag, payload: &[u8]| {
        let mut field = name(tag, "max_durability");
        field.extend(payload);
        vec![field]
    };
    for (fields, expected) in [
        (vec![], None),
        (scalar(1, &[50]), None),
        (scalar(2, &50_i16.to_le_bytes()), None),
        (scalar(4, &[100]), None),
        (scalar(5, &50_f32.to_le_bytes()), None),
        (scalar(6, &50_f64.to_le_bytes()), None),
        (vec![compound("max_durability", &[int("value", 50)])], None),
        (vec![int("max_durability", 0)], Some(0)),
        (vec![int("max_durability", -1)], None),
        (vec![int("max_durability", 50)], Some(50)),
    ] {
        let root = compound(
            "",
            &[compound(
                "components",
                &[compound("minecraft:durability", &fields)],
            )],
        );
        let facts = parse_components(&root).unwrap();
        assert!(facts.durability_declared);
        assert_eq!(facts.max_durability, expected);
    }
    let root = compound("", &[compound("components", &[])]);
    assert!(!parse_components(&root).unwrap().durability_declared);
}

#[test]
fn shooter_draw_facts_follow_the_declared_fields() {
    let shooter = |fields: &[Vec<u8>]| {
        let root = compound(
            "",
            &[compound(
                "components",
                &[compound("minecraft:shooter", fields)],
            )],
        );
        parse_components(&root).unwrap().shooter.unwrap()
    };
    let drawn = shooter(&[byte("charge_on_draw", 0), float("max_draw_duration", 1.0)]);
    assert!(drawn.starts_held_draw());
    assert!(!shooter(&[]).starts_held_draw());
    assert!(!shooter(&[float("max_draw_duration", f32::NAN)]).starts_held_draw());
    let charged = shooter(&[byte("charge_on_draw", 1), float("max_draw_duration", 1.0)]);
    assert!(charged.charge_on_draw && !charged.starts_held_draw());
    let root = compound("", &[compound("components", &[])]);
    assert_eq!(parse_components(&root).unwrap().shooter, None);
}

#[test]
fn render_offsets_reach_the_item_facts() {
    let facts = |component: Vec<u8>| {
        let root = compound("", &[compound("components", &[component])]);
        parse_components(&root).unwrap().render_offsets
    };
    assert_eq!(facts(int("unrelated", 1)), None);
    assert_eq!(
        facts(string("minecraft:render_offsets", "tools")),
        Some(render_api::DEFAULT_ITEM_RENDER_OFFSETS)
    );
}

#[test]
fn scalar_components_payload_is_not_admitted_as_empty_facts() {
    let root = compound("", &[int("components", 1)]);
    assert!(parse_components(&root).is_none());
}

// Dragonfly's custom-item layout: properties nested under item_properties.
#[test]
fn fuel_duration_distinguishes_zero_missing_and_malformed_data() {
    let missing = parse_components(&compound("", &[compound("components", &[])])).unwrap();
    assert_eq!(missing.fuel_duration_ticks, None);
    assert!(!missing.fuel_declared);
    for (seconds, expected) in [
        (2.0f32, Some(40)),
        (0.0, Some(0)),
        (-1.0, None),
        (f32::NAN, None),
    ] {
        let mut duration = name(5, "duration");
        duration.extend(seconds.to_le_bytes());
        let components = parse_components(&compound(
            "",
            &[compound(
                "components",
                &[compound("minecraft:fuel", &[duration])],
            )],
        ))
        .unwrap();
        assert_eq!(components.fuel_duration_ticks, expected);
        assert!(components.fuel_declared);
    }
    let malformed = parse_components(&compound(
        "",
        &[compound(
            "components",
            &[compound("minecraft:fuel", &[string("duration", "wrong")])],
        )],
    ))
    .unwrap();
    assert_eq!(malformed.fuel_duration_ticks, None);
    assert!(malformed.fuel_declared);
}

#[test]
fn wearable_protection_retains_zero_and_rejects_invalid_or_missing_values() {
    for (value, expected) in [
        (Some(int("protection", 6)), Some(6)),
        (Some(int("protection", 0)), Some(0)),
        (Some(int("protection", -1)), None),
        (Some(string("protection", "6")), None),
        (None, None),
    ] {
        let mut fields = vec![string("slot", "slot.armor.chest")];
        fields.extend(value);
        let payload = compound(
            "",
            &[compound(
                "components",
                &[compound("minecraft:wearable", &fields)],
            )],
        );
        let facts = parse_components(&payload).unwrap();
        assert!(facts.wearable_declared);
        assert_eq!(facts.wearable_slot.as_deref(), Some("slot.armor.chest"));
        assert_eq!(facts.wearable_protection, expected);
    }
    let absent = compound("", &[compound("components", &[])]);
    assert!(!parse_components(&absent).unwrap().wearable_declared);
}

#[test]
fn food_classification_uses_the_food_component() {
    let food = compound(
        "",
        &[compound("components", &[compound("minecraft:food", &[])])],
    );
    assert!(parse_components(&food).unwrap().food);
    let animation = compound(
        "",
        &[compound(
            "components",
            &[compound(
                "minecraft:use_animation",
                &[string("value", "eat")],
            )],
        )],
    );
    assert!(!parse_components(&animation).unwrap().food);
}

#[test]
fn reads_the_item_properties_layout() {
    let nbt = compound(
        "",
        &[compound(
            "components",
            &[
                compound(
                    "item_properties",
                    &[
                        compound(
                            "minecraft:icon",
                            &[compound("textures", &[string("default", "zeqa:sword")])],
                        ),
                        int("max_stack_size", 16),
                        byte("foil", 1),
                        byte("hand_equipped", 1),
                        int("use_animation", 1),
                        int("use_duration", 32),
                    ],
                ),
                compound(
                    "minecraft:display_name",
                    &[string("value", "item.zeqa.sword.name")],
                ),
                compound("minecraft:durability", &[int("max_durability", 250)]),
                compound("minecraft:wearable", &[string("slot", "slot.armor.head")]),
                compound("minecraft:block_placer", &[string("block", "zeqa:crate")]),
            ],
        )],
    );
    let components = parse_components(&nbt).unwrap();
    assert_eq!(components.icon.as_deref(), Some("zeqa:sword"));
    assert_eq!(
        components.display_name.as_deref(),
        Some("item.zeqa.sword.name")
    );
    assert!(components.glint == Some(true) && components.hand_equipped == Some(true));
    assert_eq!(components.max_durability, Some(250));
    assert_eq!(components.max_stack_size, Some(16));
    assert!(components.item_properties_declared);
    assert!(!components.max_stack_size_component_declared);
    assert_eq!(components.use_animation.as_deref(), Some("eat"));
    assert_eq!(components.use_duration_ticks, Some(32));
    assert_eq!(components.wearable_slot.as_deref(), Some("slot.armor.head"));
    assert_eq!(components.block_placer.as_deref(), Some("zeqa:crate"));
}

#[test]
fn duplicate_capacity_declarations_remain_ambiguous_without_losing_other_facts() {
    let icon = string("minecraft:icon", "test:gem");
    for fields in [
        vec![
            compound("item_properties", &[]),
            compound("item_properties", &[int("max_stack_size", 1)]),
            icon.clone(),
        ],
        vec![
            compound(
                "item_properties",
                &[int("max_stack_size", 64), int("max_stack_size", 1)],
            ),
            icon.clone(),
        ],
        vec![
            compound("minecraft:max_stack_size", &[int("value", 64)]),
            compound("minecraft:max_stack_size", &[int("value", 1)]),
            icon.clone(),
        ],
        vec![
            compound(
                "minecraft:max_stack_size",
                &[int("value", 64), int("value", 1)],
            ),
            icon,
        ],
    ] {
        let nbt = compound("", &[compound("components", &fields)]);
        let facts = parse_components(&nbt).unwrap();
        assert!(facts.stack_capacity_ambiguous);
        assert_eq!(facts.icon.as_deref(), Some("test:gem"));
    }
}

// Current add-on layout: top-level value components, use_modifiers in seconds.
#[test]
fn reads_top_level_value_components() {
    let nbt = compound(
        "",
        &[compound(
            "components",
            &[
                compound("minecraft:glint", &[byte("value", 1)]),
                compound("minecraft:use_animation", &[string("value", "drink")]),
                compound("minecraft:rarity", &[string("value", "epic")]),
                compound("minecraft:hover_text_color", &[string("value", "gold")]),
                compound("minecraft:hand_equipped", &[byte("value", 0)]),
                compound("minecraft:max_stack_size", &[byte("value", 1)]),
                compound(
                    "minecraft:use_modifiers",
                    &[{
                        let mut seconds = name(5, "use_duration");
                        seconds.extend(1.6f32.to_le_bytes());
                        seconds
                    }],
                ),
                compound(
                    "minecraft:block_placer",
                    &[compound("block", &[string("name", "test:ore")])],
                ),
            ],
        )],
    );
    let components = parse_components(&nbt).unwrap();
    assert!(components.glint == Some(true) && components.hand_equipped == Some(false));
    assert_eq!(components.max_stack_size, Some(1));
    assert!(components.max_stack_size_component_declared);
    assert!(!components.item_properties_declared);
    assert_eq!(components.use_duration_ticks, Some(32));
    assert_eq!(components.use_animation.as_deref(), Some("drink"));
    assert_eq!(components.rarity.as_deref(), Some("epic"));
    assert_eq!(components.hover_text_color.as_deref(), Some("gold"));
    assert_eq!(components.block_placer.as_deref(), Some("test:ore"));
    assert!(parse_components(&compound("", &[])).is_none());
}

#[test]
fn custom_content_stack_component_retains_its_own_value_with_property_precedence() {
    let payload = compound(
        "",
        &[compound(
            "components",
            &[
                compound("item_properties", &[int("max_stack_size", 64)]),
                int("minecraft:max_stack_size", 1),
            ],
        )],
    );
    let components = parse_components(&payload).unwrap();
    assert_eq!(components.max_stack_size, Some(64));
    assert_eq!(components.max_stack_size_component, Some(1));
    assert!(!components.stack_capacity_ambiguous);
}

#[test]
fn intrinsic_glint_flags_distinguish_false_missing_and_malformed_data() {
    let payload = |flag: Option<Vec<u8>>| {
        compound(
            "",
            &[compound(
                "components",
                &flag.into_iter().collect::<Vec<_>>(),
            )],
        )
    };
    let missing = parse_components(&payload(None)).unwrap();
    assert_eq!(missing.glint, None);
    assert!(!missing.glint_declared);
    let flat = parse_components(&payload(Some(compound(
        "minecraft:glint",
        &[byte("value", 0)],
    ))))
    .unwrap();
    assert_eq!(flat.glint, Some(false));
    assert!(flat.glint_declared);
    let malformed = parse_components(&payload(Some(compound("minecraft:glint", &[])))).unwrap();
    assert_eq!(malformed.glint, None);
    assert!(malformed.glint_declared);
}

#[test]
fn hand_grip_flag_presence_keeps_false_distinct_from_missing_or_malformed() {
    let payload = |flag: Option<Vec<u8>>| {
        compound(
            "",
            &[compound(
                "components",
                &flag.into_iter().collect::<Vec<_>>(),
            )],
        )
    };
    let missing = parse_components(&payload(None)).unwrap();
    assert_eq!(missing.hand_equipped, None);
    assert!(!missing.hand_equipped_declared);
    let flat = parse_components(&payload(Some(compound(
        "minecraft:hand_equipped",
        &[byte("value", 0)],
    ))))
    .unwrap();
    assert_eq!(flat.hand_equipped, Some(false));
    assert!(flat.hand_equipped_declared);
    let malformed =
        parse_components(&payload(Some(compound("minecraft:hand_equipped", &[])))).unwrap();
    assert_eq!(malformed.hand_equipped, None);
    assert!(malformed.hand_equipped_declared);
}

#[test]
fn offhand_permission_keeps_denied_missing_and_malformed_distinct() {
    let payload = |entries: &[Vec<u8>]| compound("", &[compound("components", entries)]);
    let absent = parse_components(&payload(&[])).unwrap();
    assert!(!absent.allow_off_hand_declared);
    assert_eq!(absent.allow_off_hand, None);
    for (entry, expected) in [
        (byte("minecraft:allow_off_hand", 1), Some(true)),
        (
            compound("minecraft:allow_off_hand", &[byte("value", 0)]),
            Some(false),
        ),
        (
            compound("item_properties", &[int("allow_off_hand", 1)]),
            Some(true),
        ),
        (string("minecraft:allow_off_hand", "true"), None),
        (float("minecraft:allow_off_hand", 1.0), None),
        (compound("minecraft:allow_off_hand", &[]), None),
    ] {
        let components = parse_components(&payload(&[entry])).unwrap();
        assert!(components.allow_off_hand_declared);
        assert_eq!(components.allow_off_hand, expected);
    }
    let overridden = parse_components(&payload(&[
        compound("item_properties", &[byte("allow_off_hand", 1)]),
        compound("minecraft:allow_off_hand", &[byte("value", 0)]),
    ]))
    .unwrap();
    assert_eq!(overridden.allow_off_hand, Some(false));
}
#[test]
fn dynamic_storage_requires_both_component_objects() {
    for storage in [false, true] {
        for weight in [false, true] {
            let mut fields = Vec::new();
            if storage {
                fields.push(compound("minecraft:storage_item", &[]));
            }
            if weight {
                fields.push(compound("minecraft:storage_weight_limit", &[]));
            }
            let payload = compound("", &[compound("components", &fields)]);
            assert_eq!(
                parse_components(&payload).unwrap().storage_container,
                storage && weight
            );
        }
    }
    let payload = compound(
        "",
        &[compound(
            "components",
            &[
                string("minecraft:storage_item", "wrong"),
                compound("minecraft:storage_weight_limit", &[]),
            ],
        )],
    );
    assert!(!parse_components(&payload).unwrap().storage_container);
}

/// Encodes a string list field as registry component tags are sent.
fn string_list(key: &str, values: &[&str]) -> Vec<u8> {
    let mut bytes = name(9, key);
    bytes.extend([8, u8::try_from(values.len() * 2).unwrap()]);
    for value in values {
        bytes.push(value.len() as u8);
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes
}

#[test]
fn trimmable_armor_tag_admits_the_icon_trim_artwork() {
    assert_eq!(
        crate::vanilla_tag_contains(
            super::super::TRIMMABLE_ARMORS_TAG,
            "minecraft:diamond_chestplate"
        ),
        Some(true)
    );
    let icon = compound(
        "minecraft:icon",
        &[compound(
            "textures",
            &[
                string("default", "fixture_coat"),
                string("icon_trim", "fixture_coat_trim"),
                string("gold_palette", "textures/fixture/gold"),
                string("gold_palette_spare", "textures/fixture/ignored"),
                string("iron_palette", ""),
            ],
        )],
    );
    let payload = |tags: &[&str], authored: bool| {
        let tags = if authored {
            compound("minecraft:tags", &[string_list("tags", tags)])
        } else {
            string_list("item_tags", tags)
        };
        compound("", &[compound("components", &[icon.clone(), tags])])
    };
    for authored in [false, true] {
        let trim = parse_components(&payload(&["minecraft:trimmable_armors"], authored))
            .unwrap()
            .armor_trim
            .unwrap();
        assert_eq!(trim.icon_mask.as_deref(), Some("fixture_coat_trim"));
        assert_eq!(trim.icon_palette("gold"), Some("textures/fixture/gold"));
        assert_eq!(trim.icon_palette("iron"), None);
        assert_eq!(trim.icon_palettes.len(), 1);
    }
    assert!(
        parse_components(&payload(&["minecraft:is_armor"], true))
            .unwrap()
            .armor_trim
            .is_none()
    );
    let plain = compound(
        "",
        &[compound(
            "components",
            &[compound(
                "minecraft:tags",
                &[string_list("tags", &["minecraft:trimmable_armors"])],
            )],
        )],
    );
    assert_eq!(
        parse_components(&plain).unwrap().armor_trim.as_deref(),
        Some(&super::super::CustomArmorTrim::default())
    );
}
