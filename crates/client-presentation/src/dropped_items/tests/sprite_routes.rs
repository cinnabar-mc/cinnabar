//! Carried sprite families stay separate from their placed block models.
use super::*;

#[test]
fn dropped_lantern_and_chain_variants_use_sprites_without_changing_other_block_routes() {
    let stream = stream(false, 1);
    let visuals = [
        ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
        ItemVisualRoute::RetainedBlock {
            block_runtime_id: 1,
        },
    ];
    let registry = protocol::vanilla_item_registry();
    let sprites: Vec<_> = registry
        .iter()
        .filter(|entry| {
            matches!(
                assets::facts::block_render_fact(&entry.identifier).kind,
                assets::facts::BlockRenderKind::Lantern | assets::facts::BlockRenderKind::Chain
            )
        })
        .collect();
    for family in [
        assets::facts::BlockRenderKind::Lantern,
        assets::facts::BlockRenderKind::Chain,
    ] {
        assert!(
            sprites
                .iter()
                .any(|entry| assets::facts::block_render_fact(&entry.identifier).kind == family)
        );
    }
    for entry in sprites {
        for visual in visuals {
            assert_eq!(
                item_block_id(&stream, visual, Some(&entry.identifier), None),
                None,
                "{} must use its carried sprite",
                entry.identifier,
            );
        }
    }
    for identifier in [
        "minecraft:sea_lantern",
        "minecraft:stone",
        "cinnabar:lantern",
        "cinnabar:chain",
    ] {
        for visual in visuals {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                Some((NetworkIdMode::Sequential, 1)),
                "{identifier} must keep its block route",
            );
        }
    }
    assert_eq!(
        entity_block_id(
            &stream,
            &BlockEntityKind::Falling {
                block_runtime_id: 1,
            },
        ),
        Some((NetworkIdMode::Sequential, 1, 1.0)),
    );
}

#[test]
fn dropped_carried_sprite_families_do_not_use_their_placed_models() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let entries: Vec<_> = registry
        .iter()
        .filter(|entry| {
            matches!(
                assets::facts::block_render_fact(&entry.identifier).kind,
                assets::facts::BlockRenderKind::Bamboo
                    | assets::facts::BlockRenderKind::CoralFan
                    | assets::facts::BlockRenderKind::DefaultRail
                    | assets::facts::BlockRenderKind::LeafLitter
                    | assets::facts::BlockRenderKind::ResinClump
                    | assets::facts::BlockRenderKind::TripwireHook
            )
        })
        .collect();
    assert!(!entries.is_empty());
    for entry in entries {
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
            Some("minecraft:stone"),
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

#[test]
fn dropped_attachments_use_carried_sprites_without_changing_falling_blocks() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let attachments: std::collections::BTreeSet<_> = records
        .iter()
        .filter(|record| {
            matches!(
                record.model_family,
                assets::ModelFamily::Lever
                    | assets::ModelFamily::Vine
                    | assets::ModelFamily::GlowLichen
                    | assets::ModelFamily::SculkVein
            ) && registry
                .iter()
                .any(|entry| entry.identifier.as_ref() == record.name.as_ref())
        })
        .map(|record| record.name.as_ref())
        .collect();
    assert_eq!(
        assets::facts::CARRIED_ATTACHMENT_ITEM_IDENTIFIERS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        attachments
    );
    for identifier in attachments {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                None,
                "{identifier} must use its carried sprite"
            );
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:stone"),
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

#[test]
fn dropped_flat_block_families_keep_their_carried_sprite_routes() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let entries: Vec<_> = registry
        .iter()
        .filter(|entry| {
            matches!(
                assets::facts::block_render_fact(&entry.identifier).kind,
                assets::facts::BlockRenderKind::Ladder
                    | assets::facts::BlockRenderKind::LilyPad
                    | assets::facts::BlockRenderKind::SeaPickle
            )
        })
        .collect();
    assert!(!entries.is_empty());
    for entry in entries {
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
            Some("minecraft:stone"),
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

#[test]
fn dropped_torches_use_sprites_and_keep_the_identity_guard_current() {
    let registry = protocol::vanilla_item_registry();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let torches: std::collections::BTreeSet<_> = registry
        .iter()
        .filter(|entry| {
            records.iter().any(|record| {
                record.name.as_ref() == entry.identifier.as_ref()
                    && record.model_family == assets::ModelFamily::Torch
            })
        })
        .map(|entry| entry.identifier.as_ref())
        .collect();
    assert_eq!(
        assets::facts::TORCH_ITEM_IDENTIFIERS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        torches
    );
    let stream = stream(false, 1);
    for identifier in torches {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                None,
                "{identifier}"
            );
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:stone"),
            None
        ),
        Some((NetworkIdMode::Sequential, 1)),
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

#[test]
fn dropped_powered_rails_use_sprites_and_keep_the_identity_guard_current() {
    let registry = protocol::vanilla_item_registry();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let rails: std::collections::BTreeSet<_> = registry
        .iter()
        .filter(|entry| {
            records.iter().any(|record| {
                record.name.as_ref() == entry.identifier.as_ref()
                    && record.model_family == assets::ModelFamily::Rail
                    && assets::facts::block_render_fact(&record.name).kind
                        != assets::facts::BlockRenderKind::DefaultRail
            })
        })
        .map(|entry| entry.identifier.as_ref())
        .collect();
    assert_eq!(
        assets::facts::POWERED_RAIL_ITEM_IDENTIFIERS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        rails
    );
    let stream = stream(false, 1);
    for identifier in rails {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                None,
                "{identifier}"
            );
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:stone"),
            None
        ),
        Some((NetworkIdMode::Sequential, 1)),
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

#[test]
fn dropped_flower_beds_use_sprites_and_keep_the_family_guard_current() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let flowers: std::collections::BTreeSet<_> = records
        .iter()
        .filter(|record| {
            record.model_family == assets::ModelFamily::FlowerBed
                && registry
                    .iter()
                    .any(|entry| entry.identifier.as_ref() == record.name.as_ref())
        })
        .map(|record| record.name.as_ref())
        .collect();
    assert_eq!(
        assets::facts::FLOWER_BED_ITEM_IDENTIFIERS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        flowers
    );
    for identifier in flowers {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                None,
                "{identifier} must use its carried sprite"
            );
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:stone"),
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

#[test]
fn dropped_bars_and_candles_use_sprites_and_keep_family_guards_current() {
    let stream = stream(false, 1);
    let registry = protocol::vanilla_item_registry();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for (identifiers, candles) in [
        (assets::facts::CANDLE_ITEM_IDENTIFIERS.as_slice(), true),
        (assets::facts::METAL_BAR_ITEM_IDENTIFIERS.as_slice(), false),
    ] {
        let expected: std::collections::BTreeSet<_> = records
            .iter()
            .filter(|record| {
                let member = if candles {
                    record
                        .traits
                        .contains(assets::registry::BlockTraits::CANDLE)
                } else {
                    record.model_family == assets::ModelFamily::Pane
                        && assets::facts::block_render_fact(&record.name).flat_icon
                };
                member
                    && registry
                        .iter()
                        .any(|entry| entry.identifier.as_ref() == record.name.as_ref())
            })
            .map(|record| record.name.as_ref())
            .collect();
        assert_eq!(
            identifiers
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        for identifier in expected {
            for visual in [
                ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
                ItemVisualRoute::RetainedBlock {
                    block_runtime_id: 1,
                },
            ] {
                assert_eq!(
                    item_block_id(&stream, visual, Some(identifier), None),
                    None,
                    "{identifier} must use its carried sprite"
                );
            }
        }
    }
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            Some("minecraft:stone"),
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

#[test]
fn dropped_shaped_plants_and_eggs_use_sprites_and_keep_the_identity_guard_current() {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let registry = protocol::vanilla_item_registry();
    let expected: std::collections::BTreeSet<_> = registry
        .iter()
        .filter(|item| {
            records.iter().any(|record| {
                record.name.as_ref() == item.identifier.as_ref()
                    && record.model_family == assets::ModelFamily::Unknown
                    && !record
                        .traits
                        .contains(assets::registry::BlockTraits::CANDLE)
                    && assets::facts::block_render_fact(&record.name).kind
                        == assets::facts::BlockRenderKind::Ordinary
                    && assets::facts::block_render_fact(&record.name).flat_icon
                    && record.name.as_ref() != "minecraft:spore_blossom"
            })
        })
        .map(|item| item.identifier.as_ref())
        .collect();
    assert_eq!(
        assets::facts::SHAPED_SPRITE_ITEM_IDENTIFIERS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    let stream = stream(false, 1);
    for identifier in expected {
        for visual in [
            ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: 1,
            },
        ] {
            assert_eq!(
                item_block_id(&stream, visual, Some(identifier), None),
                None,
                "{identifier}"
            );
        }
    }
    for identifier in [
        "minecraft:spore_blossom",
        "minecraft:sea_lantern",
        "minecraft:stone",
    ] {
        assert!(
            registry
                .iter()
                .any(|item| item.identifier.as_ref() == identifier)
        );
        assert_eq!(
            item_block_id(
                &stream,
                ItemVisualRoute::BlockItem(assets::BlockVisualId(1)),
                Some(identifier),
                None
            ),
            Some((NetworkIdMode::Sequential, 1)),
            "{identifier}"
        );
    }
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
