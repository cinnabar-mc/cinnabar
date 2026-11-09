use super::*;
use std::sync::Arc;

/// Synthetic and received lifetimes keep their arrival order independently of frame count.
#[test]
fn synthetic_and_network_dimensions_share_lifetime_order() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(7);
    let feed = super::local_tests::local_feed(0.0, 0.0);
    store.sync_local_player(7, 70, &feed);
    let first = store.get(7).unwrap().explicit_dimensions;
    let mut render_revision = store.get(7).unwrap().render_spawn_revision;
    assert_eq!(store.local_actor_revision(), 0);
    for _ in 0..10 {
        store.sync_local_player(7, 70, &feed);
    }
    assert_eq!(store.get(7).unwrap().explicit_dimensions, first);
    assert_eq!(store.get(7).unwrap().render_spawn_revision, render_revision);
    assert_eq!(
        store.apply(1, 1, super::tests::spawn(7, 70)),
        ActorApplyResult::Replaced
    );
    let network = store.get(7).unwrap().explicit_dimensions;
    assert_ne!(store.get(7).unwrap().render_spawn_revision, render_revision);
    render_revision = store.get(7).unwrap().render_spawn_revision;
    assert!(network.spawn_revision > first.spawn_revision);
    store.apply(
        1,
        2,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 70,
        }),
    );
    store.sync_local_player(7, 70, &feed);
    let synthetic = store.get(7).unwrap().explicit_dimensions;
    assert_ne!(store.get(7).unwrap().render_spawn_revision, render_revision);
    render_revision = store.get(7).unwrap().render_spawn_revision;
    assert!(synthetic.spawn_revision > network.spawn_revision);
    assert_eq!(
        store.apply(1, 3, super::tests::spawn(7, 70)),
        ActorApplyResult::Replaced
    );
    let current = store.get(7).unwrap().explicit_dimensions;
    assert_ne!(store.get(7).unwrap().render_spawn_revision, render_revision);
    render_revision = store.get(7).unwrap().render_spawn_revision;
    assert!(current.spawn_revision > synthetic.spawn_revision);
    assert_eq!(
        store.apply(2, 99, super::tests::spawn(7, 70)),
        ActorApplyResult::StaleSession
    );
    assert_eq!(
        store.apply(1, 3, super::tests::spawn(7, 70)),
        ActorApplyResult::StaleSequence
    );
    assert_eq!(store.get(7).unwrap().explicit_dimensions, current);
    assert_eq!(store.get(7).unwrap().render_spawn_revision, render_revision);
    assert_eq!(store.reset_dimension(1, 4, 1), ActorApplyResult::Reset);
    store.sync_local_player(7, 70, &feed);
    assert_ne!(store.get(7).unwrap().render_spawn_revision, render_revision);
    assert!(store.get(7).unwrap().explicit_dimensions.spawn_revision > current.spawn_revision);
    let previous_session = store.lifetime(7).unwrap();
    store.begin_session(2, 0);
    store.sync_local_player(7, 70, &feed);
    assert_ne!(
        store.lifetime(7).unwrap().session_id,
        previous_session.session_id
    );
}

/// Explicit dimension ownership follows accepted writes even when their values are unchanged.
#[test]
fn explicit_dimensions_keep_independent_arrivals() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = super::tests::spawn(7, 70) else {
        unreachable!()
    };
    spawn.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Height.wire_value(),
        value: ActorMetadataValue::Float(1.5),
    }]);
    store.apply(1, 10, ActorEvent::Spawn(spawn));
    assert_eq!(
        store.get(7).unwrap().explicit_dimensions.height,
        Some(DimensionWrite {
            sequence: 10,
            value: 1.5
        })
    );
    for (sequence, value, expected) in [(11, 1.5, 11), (12, f32::NAN, 11), (13, 0.6, 13)] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: ActorDataKey::Height.wire_value(),
                    value: ActorMetadataValue::Float(value),
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        assert_eq!(
            store
                .get(7)
                .unwrap()
                .explicit_dimensions
                .height
                .unwrap()
                .sequence,
            expected
        );
    }

    store.apply(
        1,
        14,
        ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: Arc::from([protocol::ActorMetadata {
                key: ActorDataKey::Width.wire_value(),
                value: ActorMetadataValue::Float(0.6),
            }]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    let dimensions = store.get(7).unwrap().explicit_dimensions;
    assert_eq!(
        dimensions.width,
        Some(DimensionWrite {
            sequence: 14,
            value: 0.6
        })
    );
    assert_eq!(
        dimensions.height,
        Some(DimensionWrite {
            sequence: 13,
            value: 0.6
        })
    );
}

/// Received dimensions stay authoritative across non-standing pose and scale updates.
#[test]
fn remote_pose_flags_do_not_transform_received_dimensions_twice() {
    use protocol_ids::ActorFlag;
    for pose in [
        ActorFlag::Sneaking,
        ActorFlag::Swimming,
        ActorFlag::Gliding,
        ActorFlag::Crawling,
        ActorFlag::Sleeping,
    ] {
        for (width, height, scale) in [(0.6, 0.6, 1.0), (0.6, 1.5, 1.0), (0.8, 1.2, 2.0)] {
            let mut store = ActorStore::new(1, 0);
            let ActorEvent::Spawn(mut spawn) = super::tests::spawn(7, 70) else {
                unreachable!()
            };
            spawn.kind = protocol::ActorKind::Player {
                uuid: [7; 16],
                username: "Remote".into(),
            };
            store.apply(1, 1, ActorEvent::Spawn(spawn));
            let feet = store.get(7).unwrap().position;
            let bit = pose.wire_value();
            let pose_metadata = protocol::ActorMetadata {
                key: if bit < 64 {
                    ActorDataKey::Flags
                } else {
                    ActorDataKey::FlagsTwo
                }
                .wire_value(),
                value: if bit < 64 {
                    ActorMetadataValue::Flags(1 << bit)
                } else {
                    ActorMetadataValue::FlagsExtended(1 << (bit - 64))
                },
            };
            for (sequence, metadata) in [
                (
                    2,
                    vec![
                        pose_metadata.clone(),
                        protocol::ActorMetadata {
                            key: ActorDataKey::Width.wire_value(),
                            value: ActorMetadataValue::Float(width),
                        },
                        protocol::ActorMetadata {
                            key: ActorDataKey::Height.wire_value(),
                            value: ActorMetadataValue::Float(height),
                        },
                        protocol::ActorMetadata {
                            key: ActorDataKey::Scale.wire_value(),
                            value: ActorMetadataValue::Float(scale),
                        },
                    ],
                ),
                (3, vec![pose_metadata]),
            ] {
                store.apply(
                    1,
                    sequence,
                    ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 7,
                        metadata: metadata.into(),
                        properties: Arc::from([]),
                        tick: sequence,
                    }),
                );
                store.advance_interpolation_ticks(1);
                let actor = store.get(7).unwrap();
                let (min, max) = actor.bounding_box().unwrap();
                assert_eq!(actor.position, feet);
                assert!(
                    (max[0] - min[0] - width * scale).abs() < 0.00001,
                    "{pose:?}"
                );
                assert!(
                    (max[1] - min[1] - height * scale).abs() < 0.00001,
                    "{pose:?}"
                );
                assert_eq!(min[1], feet[1]);
                assert_eq!(actor.hit_boxes().collect::<Vec<_>>(), [(min, max)]);
            }
        }
    }
}
