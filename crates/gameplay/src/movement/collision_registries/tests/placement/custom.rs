use super::*;
use protocol::{CustomBlock, CustomBlocks};
use sim::{Aabb, CollisionWorld, Vec3};
use std::sync::Arc;

/// Admits a single definition under both encodings and selects its held palette state.
fn admit(block: CustomBlock, mode: NetworkIdMode, index: u32) -> (PhysicsCollisionRegistries, u32) {
    let hash = block.hashed_states()[index as usize].hash;
    let custom = CustomBlocks {
        blocks: vec![block].into(),
        ..Default::default()
    };
    let mut registry = fixtures().1.clone();
    let (range, _) = registry.begin_session_custom_blocks(&custom).unwrap();
    registry.begin_session_hashed_custom_blocks(&custom);
    let held = match mode {
        NetworkIdMode::Sequential => range.start + index,
        NetworkIdMode::Hashed => hash,
    };
    (registry, held)
}

/// Supplies a custom block without special state or placement behavior.
fn block() -> CustomBlock {
    super::super::custom_block("test:predicted", 1)
}

/// Resolves custom placement against the same loaded scene as native placement tests.
fn custom_predict(
    registry: &PhysicsCollisionRegistries,
    held: u32,
    mode: NetworkIdMode,
    store: &ChunkStore,
    click: &PlacementContext<'_>,
) -> Option<crate::block_use::PredictedPlacement> {
    let world = sim::PaletteWorld::new(store, registry.registry(mode), 0, Default::default());
    predicted_placement(registry, mode, held, &world, click)
}

#[test]
fn custom_blocks_predict_locally_in_both_palette_modes() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (registry, held) = admit(block(), mode, 0);
        let store = store(mode);
        let around = surroundings();
        let prediction = custom_predict(&registry, held, mode, &store, &context(&around))
            .expect("eligible custom blocks place before a server update");
        assert_eq!(prediction.position, [8, 9, 8]);
        assert_eq!(prediction.block, held);
        assert!(prediction.additional.is_empty());
        assert_eq!(registry.block_facts(mode, held).identity, None);
    }
}

#[test]
fn custom_mining_uncertainty_does_not_reject_known_placement_bounds() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for prepared in [false, true] {
            let mut definition = block();
            Arc::make_mut(&mut definition.visual).permutations = [protocol::CustomPermutation {
                condition: "return 1;".into(),
                components: Default::default(),
                physical: protocol::CustomPhysicalComponents {
                    mining: Some(Arc::new(protocol::CustomBlockMining {
                        seconds_to_destroy: -1.0,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            }]
            .into();
            if prepared {
                definition.state_physics = [definition.base_physics()].into();
                definition.state_physics_complete = true;
            }
            let (registry, held) = admit(definition, mode, 0);
            assert!(registry.custom_block_mining(mode, held).is_none());
            let prediction = custom_predict(
                &registry,
                held,
                mode,
                &store(mode),
                &context(&surroundings()),
            )
            .expect("mining uncertainty leaves known placement bounds usable");
            assert_eq!(prediction.position, [8, 9, 8]);
            assert_eq!(prediction.block, held);
        }
    }
}

/// Encodes a custom-component flag or a deprecated placing component as network NBT.
fn placing_handler_block(flag: &str) -> CustomBlock {
    let named = |tag: u8, name: &str| {
        let mut bytes = vec![tag, u8::try_from(name.len()).unwrap()];
        bytes.extend(name.as_bytes());
        bytes
    };
    let mut bytes = named(10, "");
    bytes.extend(named(10, "components"));
    if flag == "minecraft:on_player_placing" {
        bytes.extend(named(10, flag));
        bytes.extend([0, 0, 0]);
    } else {
        bytes.extend(named(10, "minecraft:custom_components"));
        bytes.extend(named(1, flag));
        bytes.extend([1, 0, 0, 0]);
    }
    let blocks = CustomBlocks::from_definitions([("test:crumbling", bytes.as_slice())]);
    blocks.blocks[0].clone()
}

/// Server placing handlers leave collision empty until an authoritative block update.
#[test]
fn custom_server_placing_handlers_wait_for_authoritative_collision() {
    for flag in [
        "hasPlayerPlacing",
        "isV1Component",
        "minecraft:on_player_placing",
    ] {
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let (registry, held) = admit(placing_handler_block(flag), mode, 0);
            let mut scene = store(mode);
            assert!(
                custom_predict(&registry, held, mode, &scene, &context(&surroundings())).is_none(),
                "{flag} must leave the resulting state to the server"
            );
            let bounds = Aabb::new(Vec3::new(8.0, 9.0, 8.0), Vec3::new(9.0, 10.0, 9.0));
            let world =
                sim::PaletteWorld::new(&scene, registry.registry(mode), 0, Default::default());
            assert!(
                !world
                    .collision_boxes(bounds)
                    .unwrap()
                    .value
                    .contains(&bounds)
            );
            scene
                .update_block(
                    SubChunkKey::new(0, 0, 0, 0),
                    BlockUpdate::new(8, 9, 8, 0, held),
                    id(record("minecraft:air", &[]), mode),
                )
                .unwrap();
            let world =
                sim::PaletteWorld::new(&scene, registry.registry(mode), 0, Default::default());
            assert!(
                world
                    .collision_boxes(bounds)
                    .unwrap()
                    .value
                    .contains(&bounds)
            );
        }
    }
}

/// Full-cube bridge targeting uses placement history independently of collision prediction.
#[test]
fn scripted_custom_blocks_continue_bridges_without_predicted_collision() {
    use crate::block_use::{BuildIntention, LocalUse, PlacementTarget};
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (registry, held) = admit(placing_handler_block("hasPlayerPlacing"), mode, 0);
        let scene = store(mode);
        assert!(custom_predict(&registry, held, mode, &scene, &context(&surroundings())).is_none());
        let mut intention = BuildIntention::default();
        let continues = registry.block_has_build_intention(mode, held);
        intention.record(
            false,
            [8, 9, 8],
            LocalUse::Place,
            continues,
            false,
            [8.5, 9.0, 8.5],
        );
        let target = intention
            .target(
                Some(PlacementTarget {
                    position: [8, 9, 8],
                    face: 1,
                }),
                [9.5, 11.0, 8.5],
                [9.5, 7.0, 8.5],
                [0.22, 0.0, 0.0],
                false,
            )
            .unwrap();
        assert_eq!(
            target,
            PlacementTarget {
                position: [8, 9, 8],
                face: 5
            }
        );
        let destination = crate::block_use::placement_cell(target.position, target.face);
        intention.record(
            true,
            destination,
            LocalUse::Place,
            continues,
            false,
            [9.5, 9.0, 8.5],
        );
        assert_eq!(
            intention.target(
                None,
                [10.5, 11.0, 8.5],
                [10.5, 7.0, 8.5],
                [0.22, 0.0, 0.0],
                false
            ),
            Some(PlacementTarget {
                position: destination,
                face: 5
            })
        );
        let world = sim::PaletteWorld::new(&scene, registry.registry(mode), 0, Default::default());
        assert_eq!(
            world.primary_runtime_id(destination).unwrap(),
            id(record("minecraft:air", &[]), mode)
        );
    }
}

// Held bridging with a full-cube custom block, such as Hive's Crumbling Cobblestone, must
// extend the bridge along the walk like cobblestone instead of stacking on its top faces.
#[test]
fn full_cube_custom_blocks_continue_held_bridges() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (registry, held) = admit(block(), mode, 0);
        assert!(registry.block_has_build_intention(mode, held));
        let mut half = block();
        half.collision_boxes = Some(
            [protocol::CustomBox {
                min: [0.0; 3],
                max: [1.0, 0.5, 1.0],
            }]
            .into(),
        );
        let (registry, held) = admit(half, mode, 0);
        assert!(!registry.block_has_build_intention(mode, held));
    }
}

/// Combines enabled trait axes with an independent authored property.
fn trait_block() -> CustomBlock {
    use protocol::{CustomStateAxis, CustomStateValue as V};
    let axes = [
        (
            "minecraft:block_face",
            vec!["down", "up", "north", "south", "west", "east"],
        ),
        ("minecraft:vertical_half", vec!["bottom", "top"]),
        (
            "minecraft:cardinal_direction",
            vec!["south", "west", "north", "east"],
        ),
        (
            "minecraft:facing_direction",
            vec!["down", "up", "north", "south", "west", "east"],
        ),
        ("test:colour", vec!["red", "blue"]),
    ]
    .map(|(name, values)| CustomStateAxis {
        name: name.into(),
        values: values
            .into_iter()
            .map(|value| V::String(value.into()))
            .collect(),
    });
    let mut block = block();
    block.state_count = axes.iter().map(|axis| axis.values.len() as u32).product();
    let visual = Arc::make_mut(&mut block.visual);
    visual.state_axes = Box::new(axes);
    visual.placement.trait_state_count = 4;
    visual.placement.y_rotation_offset = 90.0;
    block
}

/// Reads the selected custom state using its admitted palette identity.
fn values(registry: &PhysicsCollisionRegistries, mode: NetworkIdMode, block: u32) -> Vec<String> {
    let (definition, state) = registry.custom_placement_state(mode, block).unwrap();
    definition
        .state_values(state)
        .unwrap()
        .iter()
        .map(|value| match value {
            protocol::CustomStateValue::String(value) => value.to_string(),
            other => panic!("expected fixture string: {other:?}"),
        })
        .collect()
}

#[test]
fn custom_traits_follow_click_and_yaw_without_resetting_properties() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let definition = trait_block();
        let (registry, held) = admit(definition.clone(), mode, definition.state_count - 1);
        for (face, y, expected_face, expected_half) in [
            (0, 0.0, "down", "top"),
            (1, 1.0, "up", "bottom"),
            (2, 0.5, "north", "bottom"),
            (3, 0.5001, "south", "top"),
            (4, 0.2, "west", "bottom"),
            (5, 0.8, "east", "top"),
        ] {
            let around = surroundings();
            let mut click = context(&around);
            click.input.face = face;
            click.input.click_position[1] = y;
            for (yaw, direction) in [
                (0.0, "east"),
                (90.0, "south"),
                (180.0, "west"),
                (-90.0, "north"),
            ] {
                click.input.yaw = yaw;
                let predicted =
                    custom_predict(&registry, held, mode, &store(mode), &click).unwrap();
                let selected = values(&registry, mode, predicted.block);
                assert_eq!(selected[..3], [expected_face, expected_half, direction]);
                assert_eq!(selected[4], "blue");
            }
        }
    }
}

#[test]
fn custom_facing_uses_body_height_and_strict_upper_boundary() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for (bounds, expected) in [
            (([8.2, 10.0, 8.2], [8.8, 11.8, 8.8]), "down"),
            (([8.2, 6.0, 8.2], [8.8, 8.8, 8.8]), "up"),
            (([8.2, 7.0, 8.2], [8.8, 9.0, 8.8]), "east"),
            (([10.2, 10.0, 8.2], [10.8, 11.8, 8.8]), "east"),
        ] {
            let mut around = surroundings();
            // No collision fixture: the trait's body-height choice is independent of overlap.
            around.player_box = bounds;
            let mut definition = trait_block();
            definition.collides = false;
            let (non_colliding, item) = admit(definition, mode, 0);
            let predicted =
                custom_predict(&non_colliding, item, mode, &store(mode), &context(&around))
                    .unwrap();
            assert_eq!(values(&non_colliding, mode, predicted.block)[3], expected);
        }
    }
}

#[test]
fn authored_properties_are_not_placement_traits_merely_because_of_their_names() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut definition = trait_block();
        Arc::make_mut(&mut definition.visual)
            .placement
            .trait_state_count = 0;
        let (registry, held) = admit(definition, mode, 37);
        let prediction = custom_predict(
            &registry,
            held,
            mode,
            &store(mode),
            &context(&surroundings()),
        )
        .unwrap();
        assert_eq!(prediction.block, held);
    }
}

#[test]
fn custom_filters_and_unresolved_permutation_physics_defer_to_authority() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut filtered = block();
        Arc::make_mut(&mut filtered.visual)
            .placement
            .has_placement_filter = true;
        let mut unresolved = block();
        Arc::make_mut(&mut unresolved.visual).permutations = vec![protocol::CustomPermutation {
            condition: "query.unsupported()".into(),
            components: Default::default(),
            physical: protocol::CustomPhysicalComponents {
                collision: Some(protocol::CustomCollision {
                    enabled: false,
                    boxes: None,
                }),
                ..Default::default()
            },
        }]
        .into();
        let mut incomplete = block();
        incomplete.state_count = 2;
        let mut omitted = block();
        Arc::make_mut(&mut omitted.visual).components_incomplete = true;
        for definition in [filtered, unresolved, incomplete, omitted] {
            let (registry, held) = admit(definition, mode, 0);
            assert!(
                custom_predict(
                    &registry,
                    held,
                    mode,
                    &store(mode),
                    &context(&surroundings())
                )
                .is_none()
            );
        }
        // A full array may still contain fallback bounds after a condition was skipped.
        let mut partial = block();
        let physics = partial.base_physics();
        partial.state_physics = vec![physics].into();
        Arc::make_mut(&mut partial.visual).permutations = vec![protocol::CustomPermutation {
            condition: "query.unsupported()".into(),
            components: Default::default(),
            physical: protocol::CustomPhysicalComponents {
                collision: Some(protocol::CustomCollision {
                    enabled: false,
                    boxes: None,
                }),
                ..Default::default()
            },
        }]
        .into();
        let (registry, held) = admit(partial, mode, 0);
        assert!(
            custom_predict(
                &registry,
                held,
                mode,
                &store(mode),
                &context(&surroundings())
            )
            .is_none()
        );
    }
}

#[test]
fn custom_placement_checks_chosen_state_bounds_and_world_admission() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let mut definition = block();
        definition.state_count = 2;
        let visual = Arc::make_mut(&mut definition.visual);
        visual.state_axes = vec![protocol::CustomStateAxis {
            name: "minecraft:vertical_half".into(),
            values: ["bottom", "top"]
                .map(|s| protocol::CustomStateValue::String(s.into()))
                .into(),
        }]
        .into();
        visual.placement.trait_state_count = 1;
        let bottom = protocol::CustomBox {
            min: [0.0; 3],
            max: [1.0, 0.5, 1.0],
        };
        let mut lower = definition.base_physics();
        lower.collision_boxes = Some(vec![bottom].into());
        let mut upper = lower.clone();
        upper.collision_boxes = Some(
            vec![protocol::CustomBox {
                min: [0.0, 0.5, 0.0],
                max: [1.0; 3],
            }]
            .into(),
        );
        Arc::make_mut(&mut definition.visual).permutations = vec![protocol::CustomPermutation {
            condition: "q.block_state('minecraft:vertical_half') == 'top'".into(),
            components: Default::default(),
            physical: protocol::CustomPhysicalComponents {
                collision: Some(protocol::CustomCollision {
                    enabled: true,
                    boxes: upper.collision_boxes.clone(),
                }),
                ..Default::default()
            },
        }]
        .into();
        definition.state_physics = vec![lower, upper].into();
        definition.state_physics_complete = true;
        let (registry, held) = admit(definition, mode, 1);
        let mut around = surroundings();
        around.actor_boxes.push(([8.1, 9.6, 8.1], [8.9, 9.9, 8.9]));
        let scene = store(mode);
        let click = context(&around);
        let result = custom_predict(&registry, held, mode, &scene, &click).unwrap();
        assert_ne!(result.block, held, "top-face click selects bottom bounds");
        around.actor_boxes[0] = ([8.1, 9.1, 8.1], [8.9, 9.4, 8.9]);
        assert!(custom_predict(&registry, held, mode, &scene, &context(&around)).is_none());
        around.actor_boxes.clear();
        let mut click = context(&around);
        click.build_height = 0..9;
        assert!(custom_predict(&registry, held, mode, &scene, &click).is_none());
        assert!(
            custom_predict(&registry, held, mode, &ChunkStore::new(), &context(&around)).is_none()
        );
        let mut occupied = scene;
        occupied
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(8, 9, 8, 0, id(record("minecraft:stone", &[]), mode)),
                id(record("minecraft:air", &[]), mode),
            )
            .unwrap();
        assert!(custom_predict(&registry, held, mode, &occupied, &context(&around)).is_none());
    }
}

#[test]
fn custom_prediction_is_removed_on_session_reset() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (mut registry, held) = admit(block(), mode, 0);
        registry
            .begin_session_custom_blocks(&CustomBlocks::default())
            .unwrap();
        registry.begin_session_hashed_custom_blocks(&CustomBlocks::default());
        assert!(registry.custom_placement_state(mode, held).is_none());
        assert!(
            custom_predict(
                &registry,
                held,
                mode,
                &store(mode),
                &context(&surroundings())
            )
            .is_none()
        );
    }
}
