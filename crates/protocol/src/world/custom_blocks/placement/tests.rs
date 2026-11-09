use super::super::parse_definition;
use crate::nbt_tree::test_tree::Nbt;

/// Builds an original wire component fixture without a game pack.
fn compound<const N: usize>(fields: [(&str, Nbt); N]) -> Nbt {
    Nbt::Compound(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

/// Each server placing handler disables local state prediction.
#[test]
fn placing_handlers_disable_local_state_prediction() {
    for components in [
        compound([(
            "minecraft:custom_components",
            compound([("hasPlayerPlacing", Nbt::Byte(1))]),
        )]),
        compound([(
            "minecraft:custom_components",
            compound([("isV1Component", Nbt::Byte(1))]),
        )]),
        compound([("minecraft:on_player_placing", compound([]))]),
    ] {
        let definition = parse_definition(compound([("components", components)]).view()).unwrap();
        assert!(definition.visual.placement.server_placing);
        assert!(!definition.visual.placement.has_placement_filter);
    }
    let definition = parse_definition(
        compound([(
            "components",
            compound([(
                "minecraft:custom_components",
                compound([
                    ("hasPlayerPlacing", Nbt::Byte(0)),
                    ("isV1Component", Nbt::Byte(0)),
                ]),
            )]),
        )])
        .view(),
    )
    .unwrap();
    assert!(!definition.visual.placement.server_placing);
}

/// A placing handler on any permutation disables prediction for the whole type.
#[test]
fn permutation_placing_handlers_disable_local_state_prediction() {
    let definition = parse_definition(
        compound([(
            "permutations",
            Nbt::List(vec![compound([
                ("condition", Nbt::String("q.block_state('test:on')".into())),
                (
                    "components",
                    compound([(
                        "minecraft:custom_components",
                        compound([("hasPlayerPlacing", Nbt::Byte(1))]),
                    )]),
                ),
            ])]),
        )])
        .view(),
    )
    .unwrap();
    assert!(definition.visual.placement.server_placing);
}

#[test]
fn permutation_placement_filters_apply_to_the_block_type() {
    let definition = parse_definition(
        compound([(
            "permutations",
            Nbt::List(vec![compound([
                ("condition", Nbt::String("q.block_state('test:on')".into())),
                (
                    "components",
                    compound([("minecraft:placement_filter", compound([]))]),
                ),
            ])]),
        )])
        .view(),
    )
    .unwrap();
    assert!(definition.visual.placement.has_placement_filter);
}

// Using a crafting table or a block whose permutation handles interaction must not place against it.
#[test]
fn crafting_tables_and_permutation_interact_handlers_consume_the_use() {
    let permutation = |components: Nbt| {
        compound([(
            "permutations",
            Nbt::List(vec![compound([
                ("condition", Nbt::String("q.block_state('test:on')".into())),
                ("components", components),
            ])]),
        )])
    };
    for root in [
        compound([(
            "components",
            compound([(
                "minecraft:crafting_table",
                compound([("table_name", Nbt::String("Bench".into()))]),
            )]),
        )]),
        permutation(compound([(
            "minecraft:custom_components",
            compound([("hasPlayerInteract", Nbt::Byte(1))]),
        )])),
        permutation(compound([("minecraft:crafting_table", compound([]))])),
    ] {
        assert!(parse_definition(root.view()).unwrap().player_interact);
    }
    let scalar = compound([(
        "components",
        compound([("minecraft:crafting_table", Nbt::Byte(1))]),
    )]);
    assert!(!parse_definition(scalar.view()).unwrap().player_interact);
}

#[test]
fn version_one_custom_components_declare_every_handler() {
    let definition = parse_definition(
        compound([(
            "components",
            compound([(
                "minecraft:custom_components",
                compound([("isV1Component", Nbt::Byte(1))]),
            )]),
        )])
        .view(),
    )
    .unwrap();
    assert!(definition.player_interact);
}

#[test]
fn omitted_permutations_and_physical_components_are_marked_incomplete() {
    for permutations in [
        (0..=super::super::MAX_PERMUTATIONS)
            .map(|_| compound([("condition", Nbt::String("1".into()))]))
            .collect(),
        vec![compound([
            ("condition", Nbt::String("1".into())),
            (
                "components",
                compound([(
                    "minecraft:collision_box",
                    compound([("boxes", Nbt::Byte(0))]),
                )]),
            ),
        ])],
    ] {
        let definition =
            parse_definition(compound([("permutations", Nbt::List(permutations))]).view()).unwrap();
        assert!(definition.visual.components_incomplete);
        assert!(definition.skipped > 0);
    }
    for component in ["minecraft:collision_box", "minecraft:transformation"] {
        let value = if component == "minecraft:collision_box" {
            compound([("boxes", Nbt::Byte(0))])
        } else {
            compound([("SX", Nbt::Float(f64::INFINITY))])
        };
        let definition =
            parse_definition(compound([("components", compound([(component, value)]))]).view())
                .unwrap();
        assert!(definition.visual.components_incomplete);
    }
    let complete = parse_definition(compound([]).view()).unwrap();
    assert!(!complete.visual.components_incomplete);
    // A permutation without a condition never applies, so it leaves nothing unknown.
    let conditionless =
        parse_definition(compound([("permutations", Nbt::List(vec![compound([])]))]).view())
            .unwrap();
    assert!(!conditionless.visual.components_incomplete);
    assert!(conditionless.skipped > 0);
}

#[test]
fn placement_traits_retain_offset_and_exclude_authored_properties() {
    for (offset, expected) in [(0.0, 0.0), (89.0, 90.0), (-91.0, 270.0), (0.1, 0.1)] {
        let trait_ = compound([
            (
                "enabled_states",
                Nbt::List(vec![Nbt::String("minecraft:cardinal_direction".into())]),
            ),
            ("y_rotation_offset", Nbt::Float(offset)),
        ]);
        let definition = parse_definition(
            compound([
                (
                    "traits",
                    Nbt::Compound(vec![("minecraft:placement_direction".into(), trait_)]),
                ),
                (
                    "properties",
                    Nbt::List(vec![compound([
                        ("name", Nbt::String("test:on".into())),
                        ("enum", Nbt::List(vec![Nbt::Byte(0), Nbt::Byte(1)])),
                    ])]),
                ),
            ])
            .view(),
        )
        .unwrap();
        assert_eq!(definition.visual.placement.trait_state_count, 1);
        assert_eq!(definition.visual.placement.y_rotation_offset, expected);
        assert_eq!(definition.state_count, 8);
    }
}

#[test]
fn placement_traits_compose_named_states_with_connection_flags() {
    for compound_traits in [false, true] {
        for connection_states in [
            Nbt::Int(1),
            Nbt::List(vec![Nbt::String("minecraft:cardinal_connections".into())]),
            compound([("minecraft:cardinal_connections", Nbt::Byte(1))]),
        ] {
            let connection = if compound_traits {
                compound([("enabled_states", connection_states)])
            } else {
                compound([
                    ("name", Nbt::String("minecraft:connection".into())),
                    ("enabled_states", connection_states),
                ])
            };
            let traits = [
                (
                    "minecraft:placement_direction",
                    compound([
                        (
                            "enabled_states",
                            Nbt::List(vec![Nbt::String("minecraft:cardinal_direction".into())]),
                        ),
                        ("y_rotation_offset", Nbt::Float(90.0)),
                    ]),
                ),
                ("minecraft:connection", connection),
            ];
            let traits = if compound_traits {
                compound(traits)
            } else {
                Nbt::List(traits.into_iter().map(|(_, value)| value).collect())
            };
            let definition = parse_definition(
                compound([
                    ("traits", traits),
                    (
                        "properties",
                        Nbt::List(vec![compound([
                            ("name", Nbt::String("test:on".into())),
                            ("enum", Nbt::List(vec![Nbt::Byte(0), Nbt::Byte(1)])),
                        ])]),
                    ),
                ])
                .view(),
            )
            .unwrap();
            assert_eq!(definition.state_count, 4 * 16 * 2);
            assert!(!definition.visual.state_identity_incomplete);
            assert_eq!(definition.visual.placement.trait_state_count, 5);
            assert_eq!(definition.visual.placement.y_rotation_offset, 90.0);
            let axes = &definition.visual.state_axes;
            assert_eq!(axes.len(), 6);
            assert_eq!(axes[0].name.as_ref(), "minecraft:cardinal_direction");
            assert_eq!(axes[5].name.as_ref(), "test:on");
        }
    }
}
