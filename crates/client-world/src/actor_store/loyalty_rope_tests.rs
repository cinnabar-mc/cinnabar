use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    PLAYER_NETWORK_OFFSET,
};

use super::*;

const OWNER: i64 = 10;
const TRIDENT: i64 = 20;

fn owner_spawn(position: [f32; 3]) -> ActorEvent {
    let ActorEvent::Spawn(mut owner) = super::super::tests::spawn(1, OWNER) else {
        unreachable!()
    };
    owner.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "owner".into(),
    };
    owner.position = position;
    ActorEvent::Spawn(owner)
}

fn metadata(owner: i64, returning: bool) -> Arc<[ActorMetadata]> {
    Arc::from([
        ActorMetadata {
            key: ActorDataKey::Owner.wire_value(),
            value: ActorMetadataValue::Long(owner),
        },
        ActorMetadata {
            key: ActorDataKey::Flags.wire_value(),
            value: ActorMetadataValue::Flags(
                u64::from(returning) << ActorFlag::ReturnTrident.wire_value(),
            ),
        },
    ])
}

fn projectile_spawn(identifier: &str, owner: i64) -> ActorEvent {
    let ActorEvent::Spawn(mut trident) = super::super::tests::spawn(2, TRIDENT) else {
        unreachable!()
    };
    trident.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    trident.position = [0.0, 64.0, 0.0];
    trident.metadata = metadata(owner, false);
    ActorEvent::Spawn(trident)
}

fn set_returning(store: &mut ActorStore, sequence: u64, returning: bool) {
    store.apply(
        1,
        sequence,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 2,
            metadata: metadata(OWNER, returning),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
}

fn store_with_rope() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, owner_spawn([4.0, 64.0, -2.0]));
    store.apply(1, 2, projectile_spawn("minecraft:thrown_trident", OWNER));
    store
}

#[test]
fn a_thrown_trident_spans_its_rope_to_half_a_block_below_its_owner_position() {
    let store = store_with_rope();
    let [rope] = store.loyalty_ropes(1.0)[..] else {
        panic!("one rope");
    };
    assert_eq!(rope.runtime_id, 2);
    assert_eq!(rope.trident, [0.0, 64.0, 0.0]);
    let expected_y = PLAYER_NETWORK_OFFSET - 0.5;
    for (value, want) in rope.span.iter().zip([4.0, expected_y, -2.0]) {
        assert!((value - want).abs() < 1e-5, "{:?}", rope.span);
    }
    assert_eq!(rope.return_ticks, 0);
    assert!((rope.phase - (TRIDENT as f32 + 1.0) * -0.1).abs() < 1e-6);
    // Vanilla blends the trident end with the owner's previous position, so at the start of
    // a tick the span is only the half-block drop.
    let [start] = store.loyalty_ropes(0.0)[..] else {
        panic!("one rope");
    };
    for (value, want) in start.span.iter().zip([0.0, -0.5, 0.0]) {
        assert!((value - want).abs() < 1e-5, "{:?}", start.span);
    }
    let [half] = store.loyalty_ropes(0.5)[..] else {
        panic!("one rope");
    };
    for (axis, value) in half.span.iter().enumerate() {
        let want = [2.0, 0.5 * (expected_y + 0.5) - 0.5, -1.0][axis];
        assert!((value - want).abs() < 1e-5, "{axis}: {value} vs {want}");
    }
}

#[test]
fn only_a_thrown_trident_with_a_present_owner_has_a_rope() {
    let mut missing = ActorStore::new(1, 0);
    missing.apply(1, 1, owner_spawn([4.0, 64.0, -2.0]));
    missing.apply(
        1,
        2,
        projectile_spawn("minecraft:thrown_trident", OWNER + 1),
    );
    assert!(missing.loyalty_ropes(1.0).is_empty());

    let mut arrow = ActorStore::new(1, 0);
    arrow.apply(1, 1, owner_spawn([4.0, 64.0, -2.0]));
    arrow.apply(1, 2, projectile_spawn("minecraft:arrow", OWNER));
    assert!(arrow.loyalty_ropes(1.0).is_empty());
}

#[test]
fn return_ticks_count_ticks_with_the_return_flag_and_are_kept_once_it_clears() {
    let mut store = store_with_rope();
    store.advance_interpolation_ticks(2);
    assert_eq!(store.loyalty_ropes(1.0)[0].return_ticks, 0);
    set_returning(&mut store, 3, true);
    store.advance_interpolation_ticks(3);
    assert_eq!(store.loyalty_ropes(1.0)[0].return_ticks, 3);
    set_returning(&mut store, 4, false);
    store.advance_interpolation_ticks(2);
    assert_eq!(store.loyalty_ropes(1.0)[0].return_ticks, 3);
}
