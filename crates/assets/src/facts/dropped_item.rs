//! Dropped block-item presentation, checked against the pinned block and item census.

use std::sync::OnceLock;

#[cfg(test)]
use super::{BlockRenderFact, BlockRenderKind as R, block_render_fact};
#[cfg(test)]
use crate::{ModelFamily as M, RegistryRecord, registry::BlockTraits as T};

/// The identity policy applied before inspecting a dropped item's runtime visual.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DroppedItemPresentation {
    /// Leave geometry selection to the admitted block visual.
    BlockModel,
    /// Use carried artwork even when the block visual has model geometry.
    CarriedSprite,
}

/// Declares sprite membership once and checks each group against its owning registry facts.
macro_rules! carried_sprites {
    ($($selector:expr => [$($identifier:literal),+ $(,)?]),+ $(,)?) => {
        const CARRIED_SPRITES: &[&str] = &[$($($identifier,)+)+];

        #[cfg(test)]
        #[test]
        fn dropped_item_policy_covers_exact_pinned_families() {
            use std::collections::BTreeSet;

            let items = super::ItemFacts::builtin().unwrap();
            let retail: BTreeSet<_> = items.payload().records.iter()
                .map(|record| record.identifier.as_ref()).collect();
            let records = crate::read_registry_for_protocol(
                crate::pinned_block_registry_bytes(),
                crate::active_content_registry_protocol(),
            ).unwrap();
            let declared: BTreeSet<_> = CARRIED_SPRITES.iter().copied().collect();
            assert_eq!(declared.len(), CARRIED_SPRITES.len(), "duplicate sprite policy");
            assert!(declared.is_subset(&retail), "unknown retail item in sprite policy");
            $(
                let expected: BTreeSet<_> = records.iter()
                    .filter(|record| retail.contains(record.name.as_ref()))
                    .filter(|record| ($selector)(record, block_render_fact(&record.name)))
                    .map(|record| record.name.as_ref()).collect();
                assert!(!expected.is_empty(), "empty sprite family: {}", stringify!($selector));
                assert_eq!(
                    BTreeSet::from([$($identifier),+]), expected,
                    "stale sprite family: {}", stringify!($selector),
                );
            )+
        }
    };
}

carried_sprites! {
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::Lantern => [
        "minecraft:copper_lantern",
        "minecraft:exposed_copper_lantern",
        "minecraft:lantern",
        "minecraft:oxidized_copper_lantern",
        "minecraft:soul_lantern",
        "minecraft:waxed_copper_lantern",
        "minecraft:waxed_exposed_copper_lantern",
        "minecraft:waxed_oxidized_copper_lantern",
        "minecraft:waxed_weathered_copper_lantern",
        "minecraft:weathered_copper_lantern",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::Chain => [
        "minecraft:copper_chain",
        "minecraft:exposed_copper_chain",
        "minecraft:iron_chain",
        "minecraft:oxidized_copper_chain",
        "minecraft:waxed_copper_chain",
        "minecraft:waxed_exposed_copper_chain",
        "minecraft:waxed_oxidized_copper_chain",
        "minecraft:waxed_weathered_copper_chain",
        "minecraft:weathered_copper_chain",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::Bamboo => [
        "minecraft:bamboo",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::CoralFan => [
        "minecraft:brain_coral_fan",
        "minecraft:bubble_coral_fan",
        "minecraft:dead_brain_coral_fan",
        "minecraft:dead_bubble_coral_fan",
        "minecraft:dead_fire_coral_fan",
        "minecraft:dead_horn_coral_fan",
        "minecraft:dead_tube_coral_fan",
        "minecraft:fire_coral_fan",
        "minecraft:horn_coral_fan",
        "minecraft:tube_coral_fan",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::LeafLitter => [
        "minecraft:leaf_litter",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::ResinClump => [
        "minecraft:resin_clump",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::TripwireHook => [
        "minecraft:tripwire_hook",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::Ladder => [
        "minecraft:ladder",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::LilyPad => [
        "minecraft:waterlily",
    ],
    |_: &RegistryRecord, render: BlockRenderFact| render.kind == R::SeaPickle => [
        "minecraft:sea_pickle",
    ],
    |record: &RegistryRecord, _: BlockRenderFact| record.model_family == M::Rail => [
        "minecraft:rail",
        "minecraft:golden_rail",
        "minecraft:detector_rail",
        "minecraft:activator_rail",
    ],
    |record: &RegistryRecord, _: BlockRenderFact| record.model_family == M::Torch => [
        "minecraft:torch",
        "minecraft:redstone_torch",
        "minecraft:soul_torch",
        "minecraft:copper_torch",
    ],
    |record: &RegistryRecord, _: BlockRenderFact| {
        matches!(record.model_family, M::Lever | M::Vine | M::GlowLichen | M::SculkVein)
    } => [
        "minecraft:lever",
        "minecraft:vine",
        "minecraft:glow_lichen",
        "minecraft:sculk_vein",
    ],
    |record: &RegistryRecord, _: BlockRenderFact| record.model_family == M::FlowerBed => [
        "minecraft:pink_petals",
        "minecraft:wildflowers",
    ],
    |record: &RegistryRecord, _: BlockRenderFact| record.traits.contains(T::CANDLE) => [
        "minecraft:candle",
        "minecraft:white_candle",
        "minecraft:orange_candle",
        "minecraft:magenta_candle",
        "minecraft:light_blue_candle",
        "minecraft:yellow_candle",
        "minecraft:lime_candle",
        "minecraft:pink_candle",
        "minecraft:gray_candle",
        "minecraft:light_gray_candle",
        "minecraft:cyan_candle",
        "minecraft:purple_candle",
        "minecraft:blue_candle",
        "minecraft:brown_candle",
        "minecraft:green_candle",
        "minecraft:red_candle",
        "minecraft:black_candle",
    ],
    |record: &RegistryRecord, render: BlockRenderFact| record.model_family == M::Pane && render.flat_icon => [
        "minecraft:iron_bars",
        "minecraft:copper_bars",
        "minecraft:exposed_copper_bars",
        "minecraft:weathered_copper_bars",
        "minecraft:oxidized_copper_bars",
        "minecraft:waxed_copper_bars",
        "minecraft:waxed_exposed_copper_bars",
        "minecraft:waxed_weathered_copper_bars",
        "minecraft:waxed_oxidized_copper_bars",
    ],
    |record: &RegistryRecord, render: BlockRenderFact| {
        record.model_family == M::Unknown
            && !record.traits.contains(T::CANDLE)
            && render.kind == R::Ordinary
            && render.flat_icon
            && record.name.as_ref() != "minecraft:spore_blossom"
    } => [
        "minecraft:closed_eyeblossom",
        "minecraft:open_eyeblossom",
        "minecraft:pitcher_plant",
        "minecraft:pointed_dripstone",
        "minecraft:sulfur_spike",
        "minecraft:frog_spawn",
        "minecraft:sniffer_egg",
        "minecraft:turtle_egg",
    ],
}

/// Resolves exact stock identities by binary search after one lazy, carrier-free sort.
pub fn dropped_item_presentation(identifier: &str) -> DroppedItemPresentation {
    static SORTED: OnceLock<Box<[&'static str]>> = OnceLock::new();
    let identifiers = SORTED.get_or_init(|| {
        let mut identifiers = CARRIED_SPRITES.to_vec();
        identifiers.sort_unstable();
        identifiers.into_boxed_slice()
    });
    if identifiers.binary_search(&identifier).is_ok() {
        DroppedItemPresentation::CarriedSprite
    } else {
        DroppedItemPresentation::BlockModel
    }
}

#[cfg(test)]
#[test]
fn dropped_item_policy_lookup_does_not_allocate_after_initialization() {
    use DroppedItemPresentation::{BlockModel, CarriedSprite};

    assert_eq!(dropped_item_presentation(CARRIED_SPRITES[0]), CarriedSprite);
    let (_, allocations) = test_alloc::measure(|| {
        for identifier in CARRIED_SPRITES {
            assert_eq!(dropped_item_presentation(identifier), CarriedSprite);
        }
        assert_eq!(dropped_item_presentation("fixture:lantern"), BlockModel);
    });
    assert_eq!(allocations.calls, 0);
}
