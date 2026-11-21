//! Attack damage, tool efficiency and armour lines that equipment adds below its name.

use assets::item::tool_efficiency::ToolEfficiency;

use super::{BLUE_TEXT_COLOR, TooltipLine, UiRuntime};

/// `+%d %s` and `+%d%% %s`: the pack's templates for an additive attribute modifier and for
/// one shown as a percentage.
const PLUS_KEY: &str = "attribute.modifier.plus.0";
const PLUS_FALLBACK: &str = "+%d %s";
const PLUS_PERCENT_KEY: &str = "attribute.modifier.plus.1";
const PLUS_PERCENT_FALLBACK: &str = "+%d%% %s";

/// One attribute shown as a modifier: its lang keys, tried in order, and an English fallback.
#[derive(Clone, Copy)]
struct Attribute {
    name_keys: &'static [&'static str],
    fallback: &'static str,
    percent: bool,
}

const ATTACK_DAMAGE: Attribute = Attribute {
    name_keys: &[
        "attribute.name.minecraft:attack_damage",
        "attribute.name.generic.attackDamage",
    ],
    fallback: "Attack Damage",
    percent: false,
};
const ARMOR: Attribute = Attribute {
    name_keys: &["attribute.name.generic.armor"],
    fallback: "Armor",
    percent: false,
};
const TOUGHNESS: Attribute = Attribute {
    name_keys: &["attribute.name.generic.armorToughness"],
    fallback: "Toughness",
    percent: false,
};
const KNOCKBACK_RESISTANCE: Attribute = Attribute {
    name_keys: &[
        "attribute.name.minecraft:knockback_resistance",
        "attribute.name.generic.knockbackResistance",
    ],
    fallback: "Knockback Resistance",
    percent: true,
};

/// The attribute lines for a tool's efficiency, named by the tool family.
fn efficiency_attribute(kind: ToolEfficiency) -> Attribute {
    let (name_keys, fallback): (&[&str], _) = match kind {
        ToolEfficiency::Mining => (
            &["attribute.name.generic.miningEfficiency"],
            "Mining Efficiency",
        ),
        ToolEfficiency::Chop => (
            &["attribute.name.generic.chopEfficiency"],
            "Chop Efficiency",
        ),
        ToolEfficiency::Dig => (&["attribute.name.generic.digEfficiency"], "Dig Efficiency"),
    };
    Attribute {
        name_keys,
        fallback,
        percent: false,
    }
}

/// Every modifier an item shows, in display order, with the value each carries.
fn modifiers(runtime: &UiRuntime, identifier: &str) -> impl Iterator<Item = (Attribute, u32)> {
    let components = runtime.item_components(identifier);
    let facts = assets::facts::ItemFacts::builtin()
        .ok()
        .and_then(|facts| facts.by_identifier(identifier));
    let damage = components
        .and_then(|components| components.damage)
        .or_else(|| facts.and_then(assets::item::attack_damage::vanilla_attack_damage));
    let efficiency = facts.and_then(assets::item::tool_efficiency::vanilla_tool_efficiency);
    let protection = components
        .and_then(|components| components.wearable_protection)
        .or_else(|| facts.and_then(assets::facts::ItemFact::equipment_protection));
    let material = facts.and_then(assets::facts::ItemFact::equipment_material);
    // A zero modifier is not worth a line, and unresolved data has no claim to show.
    [
        damage.map(|value| (ATTACK_DAMAGE, value)),
        efficiency.map(|(kind, value)| (efficiency_attribute(kind), value)),
        protection.map(|value| (ARMOR, value)),
        material.map(|material| (TOUGHNESS, material.toughness())),
        material.map(|material| {
            (
                KNOCKBACK_RESISTANCE,
                material.knockback_resistance_percent(),
            )
        }),
    ]
    .into_iter()
    .flatten()
    .filter(|&(_, value)| value > 0)
}

/// `+4 Attack Damage` style lines for the stats an item's equipment adds.
pub(super) fn lines<'a>(
    runtime: &'a UiRuntime,
    identifier: &'a str,
) -> impl Iterator<Item = TooltipLine> + 'a {
    modifiers(runtime, identifier).map(move |(attribute, value)| {
        let (key, fallback) = if attribute.percent {
            (PLUS_PERCENT_KEY, PLUS_PERCENT_FALLBACK)
        } else {
            (PLUS_KEY, PLUS_FALLBACK)
        };
        let template = runtime.translation(key);
        let template = template.as_deref().unwrap_or(fallback);
        let name = attribute
            .name_keys
            .iter()
            .find_map(|key| runtime.translation(key))
            .map_or_else(|| attribute.fallback.to_owned(), |name| name.to_string());
        TooltipLine {
            text: protocol::format_translation(template, &[value.to_string(), name]),
            color: BLUE_TEXT_COLOR,
        }
    })
}

#[cfg(test)]
mod tests;
