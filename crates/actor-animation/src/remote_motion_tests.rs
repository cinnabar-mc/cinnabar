use super::query::wrap_degrees;
use super::*;

#[test]
fn remote_player_turns_leave_bedrock_and_java_walk_cycles_idle() {
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "Player".into(),
        },
        ..Default::default()
    };
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store = ActorAnimationStore::with_assets(
        crate::test_support::counting_random_assets_for("minecraft:player"),
    );
    store.insert(1, 0, &actors[&1]);
    for (yaw, head_yaw) in [
        (0.0, 30.0),
        (0.0, 120.0),
        (90.0, 120.0),
        (179.0, 179.0),
        (-179.0, -179.0),
        (-90.0, -90.0),
    ] {
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = yaw;
        actor.head_yaw = head_yaw;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        let state = store.rigs.values().next().unwrap();
        let input = state.history.back().unwrap();
        assert_eq!(input.position_delta, [0.0; 3]);
        assert_eq!(input.move_speed, 0.0, "Bedrock speed at yaw {yaw}");
        assert_eq!(input.distance_moved, 0.0, "Bedrock distance at yaw {yaw}");
        assert_eq!(input.walk_distance, 0.0);
        let rig = store.get(1).unwrap();
        assert_eq!(rig.java.limb_amount, [0.0; 2], "Java amount at yaw {yaw}");
        assert_eq!(rig.java.limb_swing, [0.0; 2], "Java phase at yaw {yaw}");
    }
    let rig = store.get(1).unwrap();
    assert_ne!(rig.body_yaw, 0.0);
    assert_ne!(rig.java.body_yaw[1], 0.0);

    actors.get_mut(&1).unwrap().position[0] = 0.2;
    store.advance_tick(&actors, None, None, true, true, |_| {
        ActorTickContext::default()
    });
    let state = store.rigs.values().next().unwrap();
    let input = state.history.back().unwrap();
    assert!(input.move_speed > 0.0);
    assert!(input.distance_moved > 0.0);
    assert!(input.walk_distance > 0.0);
    let rig = store.get(1).unwrap();
    assert!(rig.java.limb_amount[1] > 0.0);
    assert!(rig.java.limb_swing[1] > 0.0);
}

/// Boats are not mobs: a standing boat faces its new rotation at once, with no body-turn lag.
#[test]
fn boats_face_their_rotation_without_mob_body_lag() {
    let actor = TestActor {
        kind: ActorKind::Entity {
            identifier: assets::facts::VanillaEntity::Boat.identifier().into(),
        },
        ..Default::default()
    };
    let mut actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store =
        ActorAnimationStore::with_assets(crate::test_support::counting_random_assets_for(
            assets::facts::VanillaEntity::Boat.identifier(),
        ));
    store.insert(1, 0, &actors[&1]);
    for yaw in [150.0, -170.0, 20.0] {
        actors.get_mut(&1).unwrap().yaw = yaw;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        assert_eq!(store.get(1).unwrap().body_yaw, yaw);
    }
}

/// A player rig without installed resources isolates the seat's body-turn policy.
fn seated_player() -> (HashMap<u64, TestActor>, ActorAnimationStore) {
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "Rider".into(),
        },
        ..Default::default()
    };
    let actors = HashMap::from([(actor.runtime_id, actor)]);
    let mut store = ActorAnimationStore::with_assets(
        crate::test_support::counting_random_assets_for("minecraft:player"),
    );
    store.insert(1, 0, &actors[&1]);
    (actors, store)
}

/// One uninterrupted unlocked ride can turn the torso without starting a walk cycle.
#[test]
fn seated_players_follow_look_without_a_rotation_lock() {
    for is_local in [false, true] {
        let (mut actors, mut store) = seated_player();
        for head_yaw in [90.0, -90.0, 170.0, -170.0] {
            let actor = actors.get_mut(&1).unwrap();
            // Remote placement may retain the mount's facing while head updates keep arriving.
            actor.yaw = if is_local { head_yaw } else { 0.0 };
            actor.head_yaw = head_yaw;
            store.advance_tick(&actors, None, is_local.then_some(1), true, true, |_| {
                ActorTickContext {
                    is_local,
                    is_riding: true,
                    ..Default::default()
                }
            });
            let rig = store.get(1).unwrap();
            assert!(
                wrap_degrees(head_yaw - rig.body_yaw).abs() < 90.0,
                "local={is_local}, look={head_yaw}, body={}",
                rig.body_yaw
            );
            let input = store.rigs.values().next().unwrap().history.back().unwrap();
            assert_eq!(input.position_delta, [0.0; 3]);
            assert_eq!(input.move_speed, 0.0);
            assert_eq!(input.distance_moved, 0.0);
            assert!(rig.java.riding);
        }
    }
}

/// An explicit seat-facing constraint still wins over the player's changing look.
#[test]
fn seated_players_keep_an_explicit_body_rotation_lock() {
    let (mut actors, mut store) = seated_player();
    for head_yaw in [90.0, -90.0, 170.0, -170.0] {
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = head_yaw;
        actor.head_yaw = head_yaw;
        store.advance_tick(&actors, None, None, true, true, |_| ActorTickContext {
            is_riding: true,
            seat_facing: Some(30.0),
            ..Default::default()
        });
        assert_eq!(store.get(1).unwrap().body_yaw, 30.0);
    }
}
