use super::super::tooltip_lines;
use super::*;
use protocol::NetworkItemStack;
use std::sync::Arc;

const BLUE: [u8; 4] = BLUE_TEXT_COLOR;

fn stack(identifier: &str) -> NetworkItemStack {
    NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == identifier)
            .unwrap()
            .network_id,
        count: 1,
        ..NetworkItemStack::default()
    }
}

/// A runtime whose server lang carries the pack's attribute templates.
fn runtime() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    let input = "attribute.modifier.plus.0=+%d %s\n\
        attribute.name.minecraft:attack_damage=Attack Damage\n\
        attribute.name.generic.armor=Armor\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input.as_bytes());
        true
    }));
    runtime
}

/// The tooltip lines below the item's name.
fn stats(runtime: &UiRuntime, identifier: &str) -> Vec<(String, [u8; 4])> {
    tooltip_lines(runtime, &stack(identifier), Some(identifier), None)
        .into_iter()
        .skip(1)
        .map(|line| (line.text, line.color))
        .collect()
}

fn line(text: &str) -> (String, [u8; 4]) {
    (text.to_owned(), BLUE)
}

#[test]
fn vanilla_weapons_and_tools_show_their_attack_damage() {
    let runtime = runtime();
    for (identifier, text) in [
        ("minecraft:wooden_sword", "+4 Attack Damage"),
        ("minecraft:netherite_sword", "+8 Attack Damage"),
        ("minecraft:golden_hoe", "+2 Attack Damage"),
        ("minecraft:iron_spear", "+3 Attack Damage"),
        ("minecraft:trident", "+8 Attack Damage"),
        ("minecraft:mace", "+5 Attack Damage"),
    ] {
        assert_eq!(stats(&runtime, identifier), [line(text)], "{identifier}");
    }
}

#[test]
fn pickaxes_axes_and_shovels_show_their_efficiency_after_their_damage() {
    let runtime = runtime();
    for (identifier, damage, efficiency) in [
        ("minecraft:wooden_pickaxe", "+2", "+2 Mining Efficiency"),
        ("minecraft:diamond_pickaxe", "+5", "+8 Mining Efficiency"),
        ("minecraft:stone_axe", "+4", "+4 Chop Efficiency"),
        ("minecraft:netherite_axe", "+7", "+9 Chop Efficiency"),
        ("minecraft:iron_shovel", "+3", "+6 Dig Efficiency"),
        ("minecraft:golden_shovel", "+1", "+12 Dig Efficiency"),
    ] {
        assert_eq!(
            stats(&runtime, identifier),
            [line(&format!("{damage} Attack Damage")), line(efficiency)],
            "{identifier}"
        );
    }
}

#[test]
fn swords_and_hoes_show_no_efficiency() {
    let runtime = runtime();
    for identifier in ["minecraft:diamond_sword", "minecraft:diamond_hoe"] {
        let lines = stats(&runtime, identifier);
        assert!(
            lines.iter().all(|(text, _)| !text.contains("Efficiency")),
            "{identifier}: {lines:?}"
        );
    }
}

#[test]
fn vanilla_armour_shows_its_protection() {
    let runtime = runtime();
    for (identifier, text) in [
        ("minecraft:leather_helmet", "+1 Armor"),
        ("minecraft:iron_chestplate", "+6 Armor"),
        ("minecraft:golden_boots", "+1 Armor"),
    ] {
        assert_eq!(stats(&runtime, identifier), [line(text)], "{identifier}");
    }
}

#[test]
fn diamond_and_netherite_armour_add_toughness_and_netherite_knockback_resistance() {
    let runtime = runtime();
    assert_eq!(
        stats(&runtime, "minecraft:diamond_leggings"),
        [line("+6 Armor"), line("+2 Toughness")]
    );
    assert_eq!(
        stats(&runtime, "minecraft:netherite_boots"),
        [
            line("+3 Armor"),
            line("+3 Toughness"),
            line("+10% Knockback Resistance")
        ]
    );
}

#[test]
fn mount_armour_shows_its_protection_and_material_modifiers() {
    let runtime = runtime();
    assert_eq!(
        stats(&runtime, "minecraft:leather_horse_armor"),
        [line("+3 Armor")]
    );
    for mount in ["horse", "nautilus"] {
        for (material, expected) in [
            ("copper", vec![line("+4 Armor")]),
            ("iron", vec![line("+5 Armor")]),
            ("golden", vec![line("+7 Armor")]),
            ("diamond", vec![line("+11 Armor"), line("+2 Toughness")]),
            (
                "netherite",
                vec![
                    line("+19 Armor"),
                    line("+3 Toughness"),
                    line("+10% Knockback Resistance"),
                ],
            ),
        ] {
            let identifier = format!("minecraft:{material}_{mount}_armor");
            assert_eq!(stats(&runtime, &identifier), expected, "{identifier}");
        }
    }
}

#[test]
fn items_without_equipment_stats_show_no_stat_lines() {
    let runtime = runtime();
    for identifier in ["minecraft:stick", "minecraft:bow", "minecraft:apple"] {
        assert_eq!(stats(&runtime, identifier), [], "{identifier}");
    }
}

#[test]
fn missing_translations_fall_back_to_english() {
    let runtime = UiRuntime::new(1);
    assert_eq!(
        stats(&runtime, "minecraft:wooden_sword"),
        [line("+4 Attack Damage")]
    );
    assert_eq!(
        stats(&runtime, "minecraft:netherite_boots"),
        [
            line("+3 Armor"),
            line("+3 Toughness"),
            line("+10% Knockback Resistance")
        ]
    );
    assert_eq!(
        stats(&runtime, "minecraft:diamond_shovel")[1],
        line("+8 Dig Efficiency")
    );
}

#[test]
fn server_components_override_vanilla_stats() {
    let mut runtime = runtime();
    runtime.set_session_items(Some(Arc::new(
        crate::ui_runtime::item_facts::SessionItemComponents::from_iter([
            (
                Arc::from("minecraft:wooden_sword"),
                protocol::ItemComponents {
                    damage: Some(12),
                    ..Default::default()
                },
            ),
            (
                Arc::from("fixture:coat"),
                protocol::ItemComponents {
                    wearable_protection: Some(7),
                    damage: Some(0),
                    ..Default::default()
                },
            ),
        ]),
    )));
    assert_eq!(
        stats(&runtime, "minecraft:wooden_sword"),
        [line("+12 Attack Damage")]
    );
    let coat = NetworkItemStack {
        network_id: 10_001,
        count: 1,
        ..NetworkItemStack::default()
    };
    let lines: Vec<_> = tooltip_lines(&runtime, &coat, Some("fixture:coat"), None)
        .into_iter()
        .skip(1)
        .map(|line| (line.text, line.color))
        .collect();
    assert_eq!(lines, [line("+7 Armor")]);
}
