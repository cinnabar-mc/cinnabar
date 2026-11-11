//! Client-facing facts a server item's registry components declare.

use std::sync::Arc;

use crate::nbt_tree::{Nbt, read_root};

pub(super) const MAX_TEXT_BYTES: usize = 256;

/// Converts component seconds to the protocol's integral simulation duration.
pub(super) fn duration_ticks(seconds: f64) -> Option<u32> {
    (seconds.is_finite() && seconds >= 0.0).then(|| {
        (seconds * f64::from(simulation_clock::TICKS_PER_SECOND))
            .round()
            .min(f64::from(u32::MAX)) as u32
    })
}

/// What the client presents from one item's components; absent facts stay `None`/`false`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemComponents {
    /// Food permission retained independently from use duration.
    pub food_can_always_eat: Option<bool>,
    /// Whether a valid `minecraft:use_modifiers` compound was declared.
    pub use_modifiers_present: bool,
    /// Finite nonnegative movement multiplier, retained by its IEEE bits.
    pub use_movement_modifier_bits: Option<u64>,
    /// Authored use-category timer, excluding attack cooldowns.
    pub use_cooldown: Option<ItemUseCooldown>,
    /// Parsed fuel duration; missing or malformed declarations stay absent.
    pub fuel_duration_ticks: Option<u32>,
    /// Whether fuel was declared, including malformed component values.
    pub fuel_declared: bool,
    /// Both storage components required to register a dynamic item container are present.
    pub storage_container: bool,
    /// Attack and kinetic presentation facts, when this item declares them.
    pub attack: Option<super::ItemAttackTiming>,
    /// `minecraft:shooter` draw facts, when declared.
    pub shooter: Option<super::ItemShooter>,
    /// `minecraft:render_offsets` tables, when declared.
    pub render_offsets: Option<render_api::ItemRenderOffsets>,
    /// `item_texture.json` key from `minecraft:icon`.
    pub icon: Option<Arc<str>>,
    /// `minecraft:display_name` value: a localization key or literal text.
    pub display_name: Option<Arc<str>>,
    /// `foil` / `minecraft:glint`.
    pub glint: Option<bool>,
    /// Presence of an intrinsic-shine declaration, including malformed values.
    pub glint_declared: bool,
    /// Explicit finite hand flag; absence or malformed data remains unresolved.
    pub hand_equipped: Option<bool>,
    /// Distinguishes a malformed declaration from an absent hand component.
    pub hand_equipped_declared: bool,
    /// Explicit offhand permission; absent or malformed values remain unresolved.
    pub allow_off_hand: Option<bool>,
    /// Distinguishes a malformed offhand declaration from an absent one.
    pub allow_off_hand_declared: bool,
    /// Category declared by the legacy Int property, narrowed to its low byte.
    /// Missing or ambiguous item-properties containers stay unresolved.
    pub creative_category: Option<u8>,
    /// Nonnegative maximum from a typed Int; missing or invalid values remain unresolved.
    pub max_durability: Option<u32>,
    /// Retains declaration presence when its maximum is absent or invalid.
    pub durability_declared: bool,
    pub max_stack_size: Option<u8>,
    /// The top-level stack component, independent from item_properties precedence.
    pub max_stack_size_component: Option<u8>,
    /// Duplicate capacity fields or ancestors make the stack capacity unresolved.
    pub stack_capacity_ambiguous: bool,
    /// The components carry an `item_properties` entry, whose stack size a component item takes.
    pub item_properties_declared: bool,
    /// The components carry a top-level `minecraft:max_stack_size` entry.
    pub max_stack_size_component_declared: bool,
    /// `use_animation` by name (`eat`, `drink`, `bow`...); the legacy enum maps 1 and 2.
    pub use_animation: Option<Arc<str>>,
    pub use_duration_ticks: Option<u32>,
    /// Whether `minecraft:food` is declared.
    pub food: bool,
    /// Present only when a food component declares finite numeric nutrition facts.
    pub food_properties: Option<FoodProperties>,
    /// `minecraft:wearable` slot, e.g. `slot.armor.head`.
    pub wearable_slot: Option<Arc<str>>,
    /// Whether the payload declares `minecraft:wearable`, including malformed declarations.
    pub wearable_declared: bool,
    /// Exact non-negative integer protection; absent or invalid values stay unresolved.
    pub wearable_protection: Option<u32>,
    /// Block `minecraft:block_placer` places.
    pub block_placer: Option<Arc<str>>,
    /// `minecraft:rarity`: `common`, `uncommon`, `rare` or `epic`.
    pub rarity: Option<Arc<str>>,
    /// `minecraft:hover_text_color` format-code name, e.g. `gold`.
    pub hover_text_color: Option<Arc<str>>,
    /// `minecraft:digger` destroy speeds, when declared.
    pub digger: Option<Arc<super::ItemDigger>>,
    /// `minecraft:seed` planting rule, when declared and resolvable.
    pub seed: Option<Arc<super::ItemSeed>>,
    /// Icon trim artwork, present only for items tagged `minecraft:trimmable_armors`.
    pub armor_trim: Option<Arc<super::CustomArmorTrim>>,
}

/// One authored use cooldown with bounded owned category text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemUseCooldown {
    pub category: Arc<str>,
    pub ticks: u32,
}

impl ItemComponents {
    /// Counts the retained component allocation and its owned text for admission budgets.
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>().saturating_add(32);
        for text in [
            self.icon.as_deref(),
            self.display_name.as_deref(),
            self.use_animation.as_deref(),
            self.wearable_slot.as_deref(),
            self.block_placer.as_deref(),
            self.rarity.as_deref(),
            self.hover_text_color.as_deref(),
            self.use_cooldown
                .as_ref()
                .map(|cooldown| &*cooldown.category),
            self.attack
                .as_ref()
                .and_then(|attack| attack.attack_cooldown.as_ref())
                .map(|cooldown| &*cooldown.category),
        ]
        .into_iter()
        .flatten()
        {
            bytes = bytes.saturating_add(text.len()).saturating_add(32);
        }
        if let Some(digger) = &self.digger {
            bytes = bytes.saturating_add(digger.retained_bytes());
        }
        if let Some(seed) = &self.seed {
            bytes = bytes.saturating_add(seed.retained_bytes());
        }
        if let Some(trim) = &self.armor_trim {
            bytes = bytes.saturating_add(trim.retained_bytes());
        }
        bytes
    }
}
/// Retained finite component-defined food values, with no guessed vanilla catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoodProperties {
    nutrition: u32,
    saturation_modifier_bits: u32,
}
impl FoodProperties {
    /// Constructs validated component or version-pinned carrier values.
    pub fn new(nutrition: u32, saturation_modifier: f32) -> Option<Self> {
        (saturation_modifier.is_finite() && saturation_modifier >= 0.0).then_some(Self {
            nutrition,
            saturation_modifier_bits: saturation_modifier.to_bits(),
        })
    }
    /// Nutrition points granted when consuming this item.
    pub const fn nutrition(&self) -> u32 {
        self.nutrition
    }
    /// Component multiplier; saturation gained is nutrition × multiplier × 2.
    pub fn saturation_modifier(&self) -> f32 {
        f32::from_bits(self.saturation_modifier_bits)
    }
}
/// Reads only explicit component values; absent or malformed fields remain unavailable.
fn food_properties(component: Option<Nbt<'_>>) -> Option<FoodProperties> {
    let component = component?;
    let nutrition = component.field("nutrition")?.number()?;
    let modifier = component.field("saturation_modifier")?.number()? as f32;
    if !nutrition.is_finite()
        || !(0.0..=u32::MAX as f64).contains(&nutrition)
        || nutrition.fract() != 0.0
        || !modifier.is_finite()
        || modifier < 0.0
    {
        return None;
    }
    Some(FoodProperties {
        nutrition: nutrition as u32,
        saturation_modifier_bits: modifier.to_bits(),
    })
}

/// Keeps absent, malformed and finite flags distinct while retaining the current OR combination.
fn declared_flag(property: Option<Nbt<'_>>, component: Option<Nbt<'_>>) -> (bool, Option<bool>) {
    let read = |value: Nbt<'_>| {
        value
            .field("value")
            .or(Some(value))
            .and_then(Nbt::number)
            .filter(|value| value.is_finite())
            .map(|value| value != 0.0)
    };
    let value = match (property, component) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => read(value),
        (Some(property), Some(component)) => read(property)
            .zip(read(component))
            .map(|(left, right)| left || right),
    };
    (property.is_some() || component.is_some(), value)
}

/// Reads both the `item_properties` and the top-level component layouts; `None` when the
/// data carries no components compound.
pub(super) fn parse_components(bytes: &[u8]) -> Option<ItemComponents> {
    let document = read_root(bytes)?;
    let root = document.root();
    // A non-compound payload carries no usable facts, not confirmed absence.
    let components = root
        .field("components")
        .filter(|components| matches!(components, Nbt::Compound(_)))?;
    let properties = components.field("item_properties");
    let stack_component = components.field("minecraft:max_stack_size");
    let stack_capacity_ambiguous = root.duplicate_field("components")
        || components.duplicate_field("item_properties")
        || components.duplicate_field("minecraft:max_stack_size")
        || properties.is_some_and(|properties| properties.duplicate_field("max_stack_size"))
        || stack_component.is_some_and(|component| component.duplicate_field("value"));
    let property = |name: &str| properties.and_then(|properties| properties.field(name));
    let component = |name: &str| components.field(name);
    let use_modifiers = component("minecraft:use_modifiers")
        .filter(|modifiers| matches!(modifiers, Nbt::Compound(_)));
    let value_of = |nbt: Nbt<'_>| nbt.field("value").or(Some(nbt)).and_then(Nbt::number);
    let text = |nbt: Option<Nbt<'_>>| {
        nbt.and_then(Nbt::as_str)
            .filter(|text| !text.is_empty() && text.len() <= MAX_TEXT_BYTES)
            .map(Arc::from)
    };
    let flag = |nbt: Option<Nbt<'_>>| nbt.and_then(value_of).is_some_and(|value| value != 0.0);
    let count = |nbt: Option<Nbt<'_>>| {
        nbt.and_then(value_of)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value.min(f64::from(u32::MAX)) as u32)
    };
    let use_duration_ticks =
        count(property("use_duration").or(component("minecraft:use_duration"))).or_else(|| {
            use_modifiers
                .and_then(|modifiers| modifiers.field("use_duration"))
                .and_then(Nbt::number)
                .map(|seconds| seconds as f32)
                .and_then(simulation_clock::whole_ticks_from_seconds)
        });
    let glint = declared_flag(
        property("foil").or(component("minecraft:foil")),
        component("minecraft:glint"),
    );
    let hand_equipped = declared_flag(
        property("hand_equipped"),
        component("minecraft:hand_equipped"),
    );
    Some(ItemComponents {
        food_can_always_eat: component("minecraft:food")
            .map(|food| flag(food.field("can_always_eat"))),
        use_modifiers_present: use_modifiers.is_some(),
        use_movement_modifier_bits: use_modifiers
            .and_then(|modifiers| modifiers.field("movement_modifier"))
            .and_then(Nbt::number)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(f64::to_bits),
        use_cooldown: component("minecraft:cooldown").and_then(|cooldown| {
            if cooldown.field("type").and_then(Nbt::as_str) == Some("attack") {
                return None;
            }
            Some(ItemUseCooldown {
                category: text(cooldown.field("category"))?,
                ticks: cooldown
                    .field("duration")
                    .and_then(Nbt::number)
                    .map(|seconds| seconds as f32)
                    .and_then(simulation_clock::whole_ticks_from_seconds)?,
            })
        }),
        fuel_duration_ticks: component("minecraft:fuel")
            .and_then(|fuel| fuel.field("duration"))
            .and_then(Nbt::number)
            .and_then(duration_ticks),
        fuel_declared: component("minecraft:fuel").is_some(),
        storage_container: ["minecraft:storage_item", "minecraft:storage_weight_limit"]
            .into_iter()
            .all(|name| component(name).is_some_and(|value| matches!(value, Nbt::Compound(_)))),
        attack: super::attack::parse_attack(components),
        shooter: super::shooter::parse_shooter(component("minecraft:shooter")),
        render_offsets: super::render_offsets::parse_render_offsets(component(
            "minecraft:render_offsets",
        )),
        icon: super::icons::icon_key(components),
        display_name: text(
            component("minecraft:display_name").and_then(|name| name.field("value")),
        ),
        glint: glint.1,
        glint_declared: glint.0,
        hand_equipped: hand_equipped.1,
        hand_equipped_declared: hand_equipped.0,
        durability_declared: component("minecraft:durability").is_some()
            || component("minecraft:max_damage").is_some(),
        allow_off_hand_declared: property("allow_off_hand").is_some()
            || component("minecraft:allow_off_hand").is_some(),
        allow_off_hand: component("minecraft:allow_off_hand")
            .or(property("allow_off_hand"))
            .and_then(|value| match value.field("value").unwrap_or(value) {
                Nbt::Byte(value) => Some(value != 0),
                Nbt::Short(value) | Nbt::Int(value) | Nbt::Long(value) => Some(value != 0),
                _ => None,
            }),
        creative_category: super::registry_properties::negotiated_creative_category(bytes),
        max_durability: match component("minecraft:durability") {
            Some(durability) => durability
                .field("max_durability")
                .and_then(Nbt::int32)
                .and_then(|maximum| u32::try_from(maximum).ok()),
            None => count(component("minecraft:max_damage")).filter(|maximum| *maximum > 0),
        },
        max_stack_size: count(property("max_stack_size").or(component("minecraft:max_stack_size")))
            .filter(|size| (1..=255).contains(size))
            .map(|size| size as u8),
        max_stack_size_component: count(component("minecraft:max_stack_size"))
            .filter(|size| (1..=255).contains(size))
            .map(|size| size as u8),
        stack_capacity_ambiguous,
        item_properties_declared: properties.is_some(),
        max_stack_size_component_declared: component("minecraft:max_stack_size").is_some(),
        use_animation: text(
            component("minecraft:use_animation").and_then(|animation| animation.field("value")),
        )
        .or_else(|| {
            match property("use_animation")
                .and_then(Nbt::number)
                .map(|value| value as i32)
            {
                Some(1) => Some("eat".into()),
                Some(2) => Some("drink".into()),
                _ => None,
            }
        }),
        use_duration_ticks,
        food: component("minecraft:food").is_some(),
        food_properties: food_properties(component("minecraft:food")),
        wearable_slot: text(
            component("minecraft:wearable").and_then(|wearable| wearable.field("slot")),
        ),
        wearable_declared: component("minecraft:wearable").is_some(),
        wearable_protection: component("minecraft:wearable")
            .and_then(|wearable| wearable.field("protection"))
            .and_then(|value| match value {
                Nbt::Short(value) | Nbt::Int(value) | Nbt::Long(value)
                    if (0..=i64::from(i32::MAX)).contains(&value) =>
                {
                    Some(value as u32)
                }
                _ => None,
            }),
        block_placer: component("minecraft:block_placer").and_then(|placer| {
            let block = placer.field("block")?;
            text(Some(block)).or_else(|| text(block.field("name")))
        }),
        rarity: text(component("minecraft:rarity").and_then(|rarity| rarity.field("value"))),
        hover_text_color: text(
            component("minecraft:hover_text_color").and_then(|color| color.field("value")),
        ),
        digger: super::digger::parse_digger(component("minecraft:digger")).map(Arc::new),
        seed: super::seed::parse_seed(component("minecraft:seed")).map(Arc::new),
        armor_trim: super::armor_trim::declared(
            components,
            &super::registry_properties::declared_item_tags(bytes),
        )
        .map(Arc::new),
    })
}

#[cfg(test)]
mod tests;
