use super::*;
use assets::entity_seats::{
    CompiledRideSeat, CompiledSeatLayout, EntitySeatAdmission, SeatRotation, SeatSelector,
};

/// Creates an original static count interval without installed carrier data.
fn seat(z: f32, min: u32, max: u32) -> CompiledRideSeat {
    CompiledRideSeat {
        position: [0.0, 1.0, z].map(|value| assets::ItemDisplayScalar::new(value).unwrap()),
        min_riders: min,
        max_riders: max,
        rotation: SeatRotation::Absent,
        lock_degrees: None,
    }
}

/// Gives the store an original checked literal seat instead of an installed behavior pack.
fn store() -> ActorStore {
    store_with_seats("minecraft:allay", Box::from([seat(0.5, 0, 2)]))
}

/// Gives `base` the supplied original seats without installed carrier data.
fn store_with_seats(base: &str, seats: Box<[CompiledRideSeat]>) -> ActorStore {
    store_with_layouts(
        base,
        Box::from([CompiledSeatLayout {
            selector: SeatSelector::Base,
            seats,
        }]),
    )
}

/// Gives `base` the supplied original seat layouts without installed carrier data.
fn store_with_layouts(base: &str, layouts: Box<[CompiledSeatLayout]>) -> ActorStore {
    let mut compiled = assets::carriers::startup::test_carriers::synthetic_entity_assets(1);
    compiled.sources = Box::from([
        assets::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [2; 32],
        },
        compiled.sources[0].clone(),
    ]);
    compiled.symbols[0].identifier = base.into();
    compiled.symbols[0].source_index = 1;
    compiled = compiled
        .with_unresolved_seats()
        .with_unresolved_interactions();
    compiled.seat_layouts[0].admission = EntitySeatAdmission::Provisional { source: 0, layouts };
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    ActorStore::new_with_entity_assets(
        1,
        0,
        std::sync::Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap()),
    )
}

/// Admits one authored custom base through the real streamed registry path.
fn advertise(store: &mut ActorStore, sequence: u64, base: &str) {
    store.apply(
        1,
        sequence,
        protocol::ActorEvent::Identifiers(protocol::ActorIdentifierRegistry {
            entries: std::sync::Arc::from([protocol::ActorIdentifier {
                identifier: "fixture:mount".into(),
                base_identifier: base.into(),
            }]),
            skipped: 0,
        }),
    );
}

/// Spawns the same network identity with an unrelated display name.
fn spawn_mount(store: &mut ActorStore, sequence: u64) {
    let protocol::ActorEvent::Spawn(mut event) = super::super::tests::spawn(8, 80) else {
        unreachable!();
    };
    event.kind = protocol::ActorKind::Entity {
        identifier: "fixture:mount".into(),
    };
    store.apply(1, sequence, protocol::ActorEvent::Spawn(event));
}

#[test]
fn seat_base_admission_refreshes_early_actors_and_keeps_the_first_definition() {
    let mut store = store();
    spawn_mount(&mut store, 1);
    assert_eq!(store.seat_bindings.get(&8), Some(&None));
    advertise(&mut store, 2, "minecraft:allay");
    assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
    advertise(&mut store, 3, "fixture:unadmitted");
    assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
    spawn_mount(&mut store, 4);
    assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
    store.begin_session(2, 0);
    assert!(store.seat_bindings.is_empty());
    assert!(store.seat_bases.is_empty());
}

/// Default seats follow boarding order, so the first rider aboard, who drives, takes the
/// first seat even when a lower id boards after it.
#[test]
fn default_seats_follow_boarding_order() {
    let mut store = store_with_seats(
        "minecraft:allay",
        Box::from([seat(0.5, 0, 2), seat(-0.5, 2, 2)]),
    );
    advertise(&mut store, 1, "minecraft:allay");
    spawn_mount(&mut store, 2);
    let mut sequence = 2;
    for rider in [20, 10] {
        sequence += 1;
        store.apply(1, sequence, super::super::tests::spawn(rider as u64, rider));
    }
    for rider in [20, 10] {
        sequence += 1;
        store.apply_link(
            1,
            sequence,
            protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 80,
                rider_unique_id: rider,
                link_type: protocol::ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            },
        );
    }
    assert_eq!(store.controlling_passenger(80), Some(20));
    let mount_z = store.snapshot_by_unique(80).unwrap().position[2];
    let (driver, _) = store.rider_seat_pose(20).unwrap();
    let (second, _) = store.rider_seat_pose(10).unwrap();
    assert!((driver[2] - (mount_z + 0.5)).abs() < 1.0e-4, "{driver:?}");
    assert!((second[2] - (mount_z - 0.5)).abs() < 1.0e-4, "{second:?}");
}

/// Authored player seats resolve scale and riding height before joining native origins.
#[test]
fn authored_player_seats_resolve_riding_height_before_origin_conversion() {
    use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue};
    for (position, scale, baby, link_first) in [[0.0, -0.2, 0.0], [1.0, 0.75, 0.5]]
        .into_iter()
        .flat_map(|position| [0.0, 1.0, 3.0].map(|scale| (position, scale)))
        .flat_map(|(position, scale)| [false, true].map(|baby| (position, scale, baby)))
        .flat_map(|(position, scale, baby)| {
            [false, true].map(|early| (position, scale, baby, early))
        })
    {
        let mut positions = Vec::new();
        for streamed in [false, true] {
            let mut authored = seat(0.0, 0, 1);
            authored.position =
                position.map(|value| assets::ItemDisplayScalar::new(value).unwrap());
            let mut store = store_with_seats("minecraft:minecart", Box::from([authored]));
            advertise(&mut store, 1, "minecraft:minecart");
            let link = protocol::ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: 80,
                rider_unique_id: 10,
                link_type: protocol::ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            };
            if link_first {
                store.apply_link(1, 2, link);
            }
            spawn_mount(&mut store, 3);
            store.actors.get_mut(&8).unwrap().metadata.insert(
                ActorDataKey::Scale.wire_value(),
                ActorMetadataValue::Float(scale),
            );
            let ActorEvent::Spawn(mut rider) = super::super::tests::spawn(10, 10) else {
                unreachable!()
            };
            rider.kind = ActorKind::Player {
                uuid: [0; 16],
                username: "Rider".into(),
            };
            let riding_offset = if baby {
                0.0
            } else {
                PLAYER_RIDING_HEIGHT_OFFSET
            };
            let mut metadata = vec![
                ActorMetadata {
                    key: ActorDataKey::Scale.wire_value(),
                    value: ActorMetadataValue::Float(2.0),
                },
                ActorMetadata {
                    key: ActorDataKey::Flags.wire_value(),
                    value: ActorMetadataValue::Flags(
                        u64::from(baby) << ActorFlag::Baby.wire_value(),
                    ),
                },
            ];
            if streamed {
                let mut resolved = position.map(|axis| axis * scale);
                resolved[1] += protocol::PLAYER_NETWORK_OFFSET + riding_offset;
                metadata.push(ActorMetadata {
                    key: ActorDataKey::SeatOffset.wire_value(),
                    value: ActorMetadataValue::Vector(resolved),
                });
            }
            rider.metadata = metadata.into();
            store.apply(1, 4, ActorEvent::Spawn(rider));
            if !link_first {
                store.apply_link(1, 5, link);
            }
            let mount = store.get(8).unwrap();
            let mut expected = position.map(|axis| axis * scale);
            expected[1] += mount.network_position_offset() + riding_offset;
            expected = std::array::from_fn(|axis| mount.position[axis] + expected[axis]);
            let local = store.rider_seat_pose(10).unwrap().0;
            for axis in 0..3 {
                assert!((local[axis] - expected[axis]).abs() < 1.0e-5, "{local:?}");
            }
            let predicted_mount = [4.0, 8.0, 12.0];
            let predicted_local = store.rider_seat_on(10, predicted_mount, 0.0).unwrap().0;
            for axis in 0..3 {
                let delta = predicted_mount[axis] - mount.position[axis];
                assert!((predicted_local[axis] - expected[axis] - delta).abs() < 1.0e-5);
            }
            store.predict_remote_motion(1);
            let predicted = store.pick_pose(10).unwrap().position;
            store.advance_interpolation_ticks(1);
            let rider = store.get(10).unwrap();
            assert_eq!(rider.position, local);
            assert_eq!(predicted, local);
            positions.push((local, rider.native_position()));
        }
        assert_eq!(
            positions[0], positions[1],
            "seat={position:?}, scale={scale}, baby={baby}, early link={link_first}"
        );
    }
}

/// A rider of a mount type with no seats, which streams no seat offset, sits at the
/// mount's native origin and follows it as it moves, whether the type has no seat
/// descriptor or its behavior authors no seat layout.
#[test]
fn a_seatless_mount_carries_its_rider_at_the_mount_origin() {
    let cases = [
        (store(), "fixture:seatless", None),
        (
            store_with_layouts("minecraft:bat", Box::from([])),
            "minecraft:bat",
            Some(0),
        ),
    ];
    for (store, identifier, binding) in cases {
        seatless_mount_carries_rider(store, identifier, binding);
    }
}

/// Seats a player on a scale-0 `identifier` mount bound to `binding`, then moves the mount.
fn seatless_mount_carries_rider(mut store: ActorStore, identifier: &str, binding: Option<u32>) {
    use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue};
    let ActorEvent::Spawn(mut mount) = super::super::tests::spawn(8, 80) else {
        unreachable!()
    };
    mount.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    mount.position = [-5.5, -58.38, 9.5];
    mount.metadata = std::sync::Arc::from([ActorMetadata {
        key: ActorDataKey::Scale.wire_value(),
        value: ActorMetadataValue::Float(0.0),
    }]);
    store.apply(1, 1, ActorEvent::Spawn(mount));
    assert_eq!(store.seat_bindings.get(&8), Some(&binding));
    let ActorEvent::Spawn(mut rider) = super::super::tests::spawn(10, 10) else {
        unreachable!()
    };
    rider.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "Rider".into(),
    };
    store.apply(1, 2, ActorEvent::Spawn(rider));
    store.apply_link(
        1,
        3,
        protocol::ActorLinkEvent {
            dimension: 0,
            ridden_unique_id: 80,
            rider_unique_id: 10,
            link_type: protocol::ActorLinkType::Rider,
            immediate: false,
            rider_initiated: true,
        },
    );
    let rider_native = |store: &ActorStore| {
        let (feet, _) = store.rider_seat_pose(10).unwrap();
        store.get(10).unwrap().native_position_at(feet).unwrap()
    };
    assert_eq!(
        rider_native(&store),
        store.get(8).unwrap().native_position()
    );
    store.apply(
        1,
        4,
        ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 8,
            position: [Some(-5.5), Some(-58.38), Some(15.53)],
            position_origin: protocol::ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: Some(90.0),
            head_yaw: None,
            on_ground: None,
            teleported: false,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: protocol::ActorInterpolation {
                ticks: 1,
                force_completion: false,
            },
        }),
    );
    store.advance_interpolation_ticks(1);
    let mount = store.get(8).unwrap().native_position();
    assert!(mount[2] > 9.5, "{mount:?}");
    assert_eq!(rider_native(&store), mount);
    assert_eq!(store.get(10).unwrap().native_position(), mount);
}

#[test]
fn an_unknown_declared_seat_base_stays_unresolved_after_actor_admission() {
    let mut store = store();
    advertise(&mut store, 1, "fixture:unadmitted");
    spawn_mount(&mut store, 2);
    assert_eq!(store.seat_bindings.get(&8), Some(&None));
    advertise(&mut store, 3, "minecraft:allay");
    assert_eq!(store.seat_bindings.get(&8), Some(&None));
}

#[test]
fn compiled_base_seats_preserve_count_intervals_and_compatibility_order() {
    let admission = EntitySeatAdmission::Provisional {
        source: 0,
        layouts: Box::from([CompiledSeatLayout {
            selector: SeatSelector::Base,
            seats: Box::from([seat(0.5, 0, 2), seat(-0.5, 2, 2)]),
        }]),
    };
    assert_eq!(
        compiled_seat(&admission, 1, 0).unwrap().position,
        [0.0, 1.0, 0.5]
    );
    assert_eq!(
        compiled_seat(&admission, 2, 1).unwrap().position,
        [0.0, 1.0, -0.5]
    );
    assert_eq!(
        compiled_seat(&admission, 2, 7).unwrap().position,
        [0.0, 1.0, -0.5]
    );
    assert_eq!(compiled_seat(&admission, 3, 0), None);
}

#[test]
fn unknown_group_or_rotation_does_not_silently_use_a_base_or_zero_angle() {
    let admission = EntitySeatAdmission::Provisional {
        source: 0,
        layouts: Box::from([
            CompiledSeatLayout {
                selector: SeatSelector::Base,
                seats: Box::from([seat(0.5, 0, 2)]),
            },
            CompiledSeatLayout {
                selector: SeatSelector::UnresolvedComponentGroup { group: 2 },
                seats: Box::from([seat(2.0, 0, 2)]),
            },
        ]),
    };
    assert_eq!(compiled_seat(&admission, 1, 0), None);
    let mut expression_seat = seat(0.5, 0, 2);
    expression_seat.rotation = SeatRotation::Expression { expression: 0 };
    let admission = EntitySeatAdmission::Provisional {
        source: 0,
        layouts: Box::from([CompiledSeatLayout {
            selector: SeatSelector::Base,
            seats: Box::from([expression_seat]),
        }]),
    };
    assert_eq!(compiled_seat(&admission, 1, 0), None);
    assert_eq!(compiled_seat(&EntitySeatAdmission::Unresolved, 1, 0), None);
}
