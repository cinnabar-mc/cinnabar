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
    assert_eq!((registry.len(), sprites), (1590, 84));
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "f3b7e9715892caf7a3d90d7accf574941ebd44881ff5a297af5a943006b543c8"
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
        "minecraft:amethyst_cluster",
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
