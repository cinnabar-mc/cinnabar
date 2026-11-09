//! Minecart display blocks follow the drawn body's placement and synced overrides.

use super::*;
use render_api::PoseGeneration;

const CART: u64 = 17;

/// Publishes one cart without requiring installed pack artwork or a running server.
fn cart(identifier: &str, metadata: Vec<protocol::ActorMetadata>) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                runtime_id: CART,
                unique_id: CART as i64,
                kind: entity(identifier),
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: metadata.into(),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    test_time::eventually("minecart fixture admission", || {
        stream.poll([0.0, 64.0, 0.0], 0);
        stream.authority().actor(CART).is_some()
    });
    stream
}

/// Resolves the production additional-rendering path for an already drawn body.
fn displays(stream: &WorldStream) -> Vec<CarriedBlock> {
    let authority = stream.authority();
    let body = crate::presentation::actors::local_diagnostic_presentation(
        authority.actor_session_id(),
        0,
        CART,
        PoseGeneration::new(1),
        [10.0, 64.0, 20.0],
        45.0,
        0.0,
    )
    .unwrap()
    .submission;
    let batch = ActorPresentationBatch {
        submissions: vec![body],
        skin_layers: Vec::new(),
        artwork: HashMap::new(),
    };
    let mut frame = authority.actor_render_frame(0.5);
    carried_blocks_for(&batch, stream, &mut frame, 0.5)
}

/// A typed synced display override, including hashed runtime IDs represented as signed Ints.
fn custom(block: i32) -> Vec<protocol::ActorMetadata> {
    vec![
        protocol::ActorMetadata {
            key: ActorDataKey::CustomDisplay.wire_value(),
            value: ActorMetadataValue::Byte(1),
        },
        protocol::ActorMetadata {
            key: ActorDataKey::DisplayBlockState.wire_value(),
            value: ActorMetadataValue::Int(block),
        },
        protocol::ActorMetadata {
            key: ActorDataKey::DisplayOffset.wire_value(),
            value: ActorMetadataValue::Int(6),
        },
    ]
}

#[test]
fn hopper_minecart_draws_its_hopper_without_a_custom_display_override() {
    let draws = displays(&cart("minecraft:hopper_minecart", Vec::new()));
    let [draw] = draws.as_slice() else {
        panic!("a placed hopper minecart must draw its hopper");
    };
    let CarriedBlockId::Pinned(id) = draw.block else {
        panic!("the default hopper comes from the pinned block registry");
    };
    assert_eq!(
        assets::pinned_block_identity(id).unwrap().0,
        "minecraft:hopper"
    );
    assert_eq!(draw.light, (15, 15));
    let mut metadata = custom(-2);
    metadata[2].value = ActorMetadataValue::Int(minecart::HOPPER_DISPLAY_OFFSET);
    let explicit = displays(&cart("minecraft:hopper_minecart", metadata));
    assert_eq!(draw.world_from_block, explicit[0].world_from_block);
}

#[test]
fn minecart_custom_display_preserves_the_wire_runtime_id() {
    for identifier in ["minecraft:minecart", "minecraft:hopper_minecart"] {
        let draws = displays(&cart(identifier, custom(-2)));
        assert_eq!(draws.len(), 1, "{identifier}");
        assert_eq!(draws[0].block, CarriedBlockId::Wire(u32::MAX - 1));
    }
}

#[test]
fn a_plain_cart_and_an_unrelated_actor_do_not_gain_default_contents() {
    assert!(displays(&cart("minecraft:minecart", Vec::new())).is_empty());
    assert!(displays(&cart("fixture:hopper_minecart", custom(42))).is_empty());
    assert!(displays(&cart("minecraft:zombie", custom(42))).is_empty());
}

#[test]
fn minecart_display_toggle_restores_constructor_contents_and_placement() {
    for identifier in ["minecraft:chest_minecart", "minecraft:hopper_minecart"] {
        let default = displays(&cart(identifier, Vec::new()));
        assert_eq!(default.len(), 1, "{identifier}");
        let mut metadata = custom(-2);
        metadata[2].value = ActorMetadataValue::Int(32);
        let mut stream = cart(identifier, metadata);
        let overridden = displays(&stream);
        assert_eq!(overridden.len(), 1, "{identifier}");
        assert_eq!(overridden[0].block, CarriedBlockId::Wire(u32::MAX - 1));
        assert_ne!(default[0].world_from_block, overridden[0].world_from_block);

        for (sequence, enabled) in [(2, 0), (3, 1), (4, 0)] {
            stream
                .submit(
                    sequence,
                    WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: CART,
                        metadata: Arc::from([protocol::ActorMetadata {
                            key: ActorDataKey::CustomDisplay.wire_value(),
                            value: ActorMetadataValue::Byte(enabled),
                        }]),
                        properties: Arc::from([]),
                        tick: sequence,
                    })),
                )
                .unwrap();
            test_time::eventually("minecart display toggle admission", || {
                stream.poll([0.0, 64.0, 0.0], 0);
                stream
                    .authority()
                    .actor(CART)
                    .unwrap()
                    .metadata
                    .get(&ActorDataKey::CustomDisplay.wire_value())
                    == Some(&ActorMetadataValue::Byte(enabled))
            });
            assert_eq!(
                &displays(&stream),
                if enabled == 0 { &default } else { &overridden },
                "{identifier}, custom display {enabled}"
            );
        }
    }
}

#[test]
fn minecart_display_requires_typed_custom_metadata() {
    let mut metadata = custom(42);
    metadata[1].value = ActorMetadataValue::Long(42);
    assert!(displays(&cart("minecraft:hopper_minecart", metadata)).is_empty());
    let mut metadata = custom(42);
    metadata.remove(1);
    assert!(displays(&cart("minecraft:hopper_minecart", metadata)).is_empty());
}

#[test]
fn minecart_display_offset_moves_contents_along_the_body_up_axis() {
    let original = displays(&cart("minecraft:hopper_minecart", custom(42)));
    let mut metadata = custom(42);
    metadata[2].value = ActorMetadataValue::Int(-10);
    let lowered = displays(&cart("minecraft:hopper_minecart", metadata));
    assert_eq!(original.len(), 1);
    assert_eq!(lowered.len(), 1);
    assert_eq!(
        original[0].world_from_block[1][3] - lowered[0].world_from_block[1][3],
        1.0
    );
    for row in [0, 2] {
        assert_eq!(
            original[0].world_from_block[row],
            lowered[0].world_from_block[row]
        );
    }
}

#[test]
fn minecart_advertised_native_base_supplies_default_contents() {
    let mut stream = cart("fixture:cart", Vec::new());
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Identifiers(protocol::ActorIdentifierRegistry {
                entries: Arc::from([protocol::ActorIdentifier {
                    identifier: "fixture:cart".into(),
                    base_identifier: "minecraft:hopper_minecart".into(),
                }]),
                skipped: 0,
            })),
        )
        .unwrap();
    test_time::eventually("minecart native base admission", || {
        stream.poll([0.0, 64.0, 0.0], 0);
        stream
            .authority()
            .actor(CART)
            .unwrap()
            .entity_fact
            .class_key
            == assets::facts::entity::EntityFacts::pinned()
                .get("minecraft:hopper_minecart")
                .class_key
    });
    assert_eq!(displays(&stream).len(), 1);
}
