use super::combat_tests::fixture;
use super::*;
use protocol::{ActorEvent, ActorKind, WorldEvent};
use std::sync::Arc;

/// Admits one remote actor at `position` with the given attributes.
fn spawn(
    authority: &mut client_world::WorldAuthority,
    sequence: u64,
    (runtime_id, unique_id): (u64, i64),
    kind: ActorKind,
    position: [f32; 3],
    attributes: Vec<protocol::ActorAttribute>,
) {
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id,
                runtime_id,
                kind,
                position,
                velocity: [0.; 3],
                pitch: 0.,
                yaw: 0.,
                head_yaw: 0.,
                body_yaw: 0.,
                held_item: Default::default(),
                metadata: [].into(),
                attributes: attributes.into(),
                properties: [].into(),
                links: [].into(),
            })),
            Some(sequence),
        )
        .unwrap();
}

/// A remote player with a distinct identity.
fn player(runtime_id: u64) -> ActorKind {
    ActorKind::Player {
        uuid: [runtime_id as u8; 16],
        username: format!("player-{runtime_id}").into(),
    }
}

fn zombie() -> ActorKind {
    ActorKind::Entity {
        identifier: "minecraft:zombie".into(),
    }
}

/// A non-player row carries every fact the removed mob read reported: runtime and unique IDs,
/// type identifier, position, and replicated health with its maximum.
#[test]
fn non_player_rows_carry_every_former_mob_fact() {
    let (mut authority, world) = fixture();
    let health = protocol::ActorAttribute {
        name: protocol_ids::Attribute::Health.wire_name().into(),
        min: 0.,
        max: 400.,
        current: 250.,
        default: None,
        modifiers: Arc::from([]),
    };
    let kind = ActorKind::Entity {
        identifier: "cinnabar:hollow_warden".into(),
    };
    spawn(&mut authority, 1, (3, -9), kind, [5., 0., 0.], vec![health]);
    let rows = observed_actors(&authority, &world, 1., None);
    let [row] = rows.as_slice() else {
        panic!("expected one row, got {}", rows.len())
    };
    assert!(!row.actor.player);
    assert_eq!((row.actor.runtime_id, row.unique_id), (3, -9));
    assert_eq!(row.actor.identifier, "cinnabar:hollow_warden");
    assert_eq!(row.actor.position, (5., 0., 0.));
    assert_eq!((row.actor.health, row.max_health), (Some(250.), Some(400.)));
}

/// Nearer players fill only their own kind's rows: the nearest other actors stay readable
/// behind them, and a reader of both kinds still gets the overall nearest prefix.
#[test]
fn each_kind_keeps_its_nearest_rows_behind_nearer_actors_of_the_other_kind() {
    let (mut authority, world) = fixture();
    let players = mod_api::MAX_OBSERVED_ACTORS as u64 + 2;
    for runtime_id in 2..2 + players {
        let position = [runtime_id as f32, 0., 0.];
        spawn(
            &mut authority,
            runtime_id,
            (runtime_id, runtime_id as i64),
            player(runtime_id),
            position,
            vec![],
        );
    }
    for (index, runtime_id) in [100_u64, 101].into_iter().enumerate() {
        let position = [0., 0., 60. + index as f32];
        let id = (runtime_id, runtime_id as i64);
        spawn(&mut authority, runtime_id, id, zombie(), position, vec![]);
    }
    let rows = observed_actors(&authority, &world, 1., None);
    let ids = |player: bool| -> Vec<u64> {
        rows.iter()
            .filter(|row| row.actor.player == player)
            .map(|row| row.actor.runtime_id)
            .take(mod_api::MAX_OBSERVED_ACTORS)
            .collect()
    };
    assert_eq!(ids(false), [100, 101]);
    let nearest: Vec<_> = (2..2 + mod_api::MAX_OBSERVED_ACTORS as u64).collect();
    assert_eq!(ids(true), nearest);
    let both: Vec<_> = rows
        .iter()
        .take(mod_api::MAX_OBSERVED_ACTORS)
        .map(|row| row.actor.runtime_id)
        .collect();
    assert_eq!(both, nearest);
}

/// Rows report the pose presented at the frame's actor partial tick, which the callback reading
/// them also draws, rather than the latest received target.
#[test]
fn rows_report_the_pose_presented_at_the_frame_partial_tick() {
    let (mut authority, world) = fixture();
    spawn(&mut authority, 1, (2, 2), player(2), [0., 0., 2.], vec![]);
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 2,
                position: [Some(8.), Some(0.), Some(2.)],
                position_origin: protocol::ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: false,
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
            })),
            Some(2),
        )
        .unwrap();
    authority.advance_actor_interpolation_ticks(1);
    let actor = authority.actor(2).unwrap();
    let (previous, latest, _) = authority.pick_motion(actor);
    assert_ne!(
        previous, latest,
        "the fixture must leave the actor mid-motion"
    );
    let presented = actor.interpolated_position(0.5).unwrap();
    let rows = observed_actors(&authority, &world, 0.5, None);
    let position = rows[0].actor.position;
    assert_eq!([position.0, position.1, position.2], presented);
    assert_ne!(presented, latest);
}
