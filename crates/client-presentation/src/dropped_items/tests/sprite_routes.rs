//! Carried sprite families stay separate from their placed block models.
use super::*;

#[test]
fn pinned_dropped_item_classification_is_unchanged() {
    use sha2::{Digest, Sha256};

    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let mut identifiers: Vec<_> = registry.iter().map(|entry| &entry.identifier).collect();
    identifiers.sort_unstable();
    let mut digest = Sha256::new();
    let mut sprites = 0;
    for identifier in identifiers {
        let sprite = item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some(identifier),
            None,
        )
        .is_none();
        sprites += usize::from(sprite);
        digest.update(identifier.as_bytes());
        digest.update(if sprite { b"\t1\n" } else { b"\t0\n" });
    }
    // Freeze the complete classification, including every default block route.
    assert_eq!((registry.len(), sprites), (1590, 89));
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "f5969237e99418dec9e515f453e683e7ae37da3412f042e9ca60dc3e7d3d4b14"
    );
}

#[test]
fn declared_sprite_policy_routes_both_item_sources_in_both_id_modes() {
    use assets::facts::{DroppedItemPresentation, dropped_item_presentation};

    let registry = protocol::vanilla_item_registry();
    for hashes in [false, true] {
        let stream = stream(hashes, 7);
        let retained_mode = if hashes {
            NetworkIdMode::Hashed
        } else {
            NetworkIdMode::Sequential
        };
        for entry in registry.iter() {
            let sprite = dropped_item_presentation(&entry.identifier)
                == DroppedItemPresentation::CarriedSprite;
            for (visual, block) in [
                (
                    ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
                    (NetworkIdMode::Sequential, 1),
                ),
                (
                    ItemVisualRoute::RetainedBlock {
                        block_runtime_id: 1,
                    },
                    (retained_mode, if hashes { 1 } else { 7 }),
                ),
            ] {
                assert_eq!(
                    item_block_id(&stream, visual, Some(&entry.identifier), None),
                    (!sprite).then_some(block),
                    "{} ({visual:?}, {retained_mode:?})",
                    entry.identifier,
                );
            }
        }
        assert_eq!(
            entity_block_id(
                &stream,
                &BlockEntityKind::Falling {
                    block_runtime_id: 1
                },
            ),
            Some((retained_mode, if hashes { 1 } else { 7 }, 1.0)),
        );
    }
}

#[test]
fn sprite_policy_preserves_special_cases_and_custom_namespaces() {
    let stream = stream(false, 1);
    for identifier in [
        "minecraft:spore_blossom",
        "minecraft:sea_lantern",
        "minecraft:stone",
        "minecraft:amethyst_block",
        "minecraft:budding_amethyst",
        "cinnabar:amethyst_cluster",
        "minecraft:unlit_redstone_torch",
        "cinnabar:lantern",
        "cinnabar:chain",
        "cinnabar:candle",
        "cinnabar:torch",
        "cinnabar:iron_bars",
    ] {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                Some((NetworkIdMode::Sequential, 1)),
                "{identifier}",
            );
        }
    }
}

#[test]
fn dropped_crystal_variants_use_their_flat_carried_artwork() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let crystals: Vec<_> = registry
        .iter()
        .filter(|entry| {
            assets::facts::block_render_fact(&entry.identifier).kind
                == assets::facts::BlockRenderKind::Crystal
        })
        .collect();
    assert!(!crystals.is_empty());
    for entry in crystals {
        assert!(
            assets::facts::block_render_fact(&entry.identifier).flat_icon,
            "{} must declare its carried presentation",
            entry.identifier
        );
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(&entry.identifier), None),
                None,
                "{}",
                entry.identifier
            );
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:amethyst_block"),
            None
        ),
        Some((NetworkIdMode::Sequential, 1))
    );
    assert_eq!(
        entity_block_id(
            &stream,
            &BlockEntityKind::Falling {
                block_runtime_id: 1
            }
        ),
        Some((NetworkIdMode::Sequential, 1, 1.0))
    );
}
