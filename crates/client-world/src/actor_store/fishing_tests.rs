use super::*;
use crate::actor_store::ActorStore;
use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorStatusEvent};

/// Spawns runtime actor 7 as `identifier` with original hook dimensions, fish data and flags.
fn actor(identifier: &str, silent: bool) -> ActorEvent {
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(7, 17) else {
        unreachable!()
    };
    spawn.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    spawn.position = [4.25, 62.75, -3.5];
    let float = |key: ActorDataKey, value| ActorMetadata {
        key: key.wire_value(),
        value: ActorMetadataValue::Float(value),
    };
    spawn.metadata = std::sync::Arc::from([
        float(ActorDataKey::Width, 0.25),
        float(ActorDataKey::Height, 0.25),
        float(ActorDataKey::FishX, 1.5),
        float(ActorDataKey::FishZ, -2.0),
        float(ActorDataKey::FishAngle, 0.75),
        ActorMetadata {
            key: ActorDataKey::Flags.wire_value(),
            value: ActorMetadataValue::Flags(u64::from(silent) << ActorFlag::Silent.wire_value()),
        },
    ]);
    ActorEvent::Spawn(spawn)
}

fn notice(spawn: ActorEvent, kind: ActorStatusKind) -> crate::ActorStatusNotice {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn);
    store.apply(
        1,
        2,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind,
            data: 0,
        }),
    );
    let mut notices = store.take_status_notices();
    assert_eq!(notices.len(), 1);
    notices.pop().unwrap()
}

#[test]
fn fishing_events_carry_the_hook_box_and_synced_fish() {
    for kind in [
        ActorStatusKind::FishHookPosition,
        ActorStatusKind::FishHookBite,
        ActorStatusKind::FishHookTease,
    ] {
        for silent in [false, true] {
            let notice = notice(actor("minecraft:fishing_hook", silent), kind);
            assert_eq!(notice.position, [4.25, 62.75, -3.5]);
            assert_eq!(
                notice.fishing,
                Some(FishingHookPose {
                    identifier: "minecraft:fishing_hook".into(),
                    bottom: 62.75,
                    width: 0.25,
                    fish_offset: [1.5, -2.0],
                    fish_angle: 0.75,
                    silent,
                })
            );
        }
    }
}

#[test]
fn other_actors_and_kinds_carry_no_hook_state() {
    let cow = notice(actor("minecraft:cow", false), ActorStatusKind::FishHookBite);
    assert_eq!(cow.fishing, None);
    let hurt = notice(
        actor("minecraft:fishing_hook", false),
        ActorStatusKind::Hurt,
    );
    assert_eq!(hurt.fishing, None);
}
