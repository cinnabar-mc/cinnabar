use super::*;
use protocol::{ActorEvent, ActorStatusKind, WorldEvent};

/// A local actor and its observation scope, independent of game assets and input capture.
fn fixture() -> (client_world::WorldAuthority, WorldObservation) {
    let mut authority = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.; 3],
        None,
    );
    authority.sync_local_player_pose(&client_world::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [0; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Standard(render_api::StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: vec![0; 64 * 64 * 4].into(),
        }),
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    });
    let world = WorldObservation {
        session: 7,
        dimension: 0,
        world: "fixture".into(),
        runtime_id: 1,
        position: (0., 0., 0.),
        alive: true,
    };
    (authority, world)
}

/// Commits one local status without running an observer callback.
fn status(authority: &mut client_world::WorldAuthority, sequence: u64, kind: ActorStatusKind) {
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Status(protocol::ActorStatusEvent {
                runtime_id: 1,
                kind,
                data: 0,
            })),
            Some(sequence),
        )
        .unwrap();
}

/// Batched packets retain their own lives through collection, cursor assignment and eviction.
#[test]
fn collection_preserves_committed_lives_and_evicts_them_with_events() {
    let (mut authority, world) = fixture();
    let mut observer = ModuleObserver::default();
    observer.bind_scope((world.session, world.dimension), 0, 0);
    let before = authority.actor(1).unwrap().life().life;
    status(&mut authority, 1, ActorStatusKind::TotemActivate);
    status(&mut authority, 2, ActorStatusKind::Death);
    let after = authority.actor(1).unwrap().life().life;
    status(&mut authority, 3, ActorStatusKind::SpawnAlive);
    status(&mut authority, 4, ActorStatusKind::TotemActivate);
    assert_ne!(before, after);
    observer.collect_combat(&authority, &world);
    assert_eq!(
        observer
            .history
            .iter()
            .map(|recorded| recorded.life)
            .collect::<Vec<_>>(),
        [Some(before), Some(after), Some(after)]
    );
    let old_sequence = observer.history.front().unwrap().event.sequence;
    observer.collect_combat(&authority, &world);
    assert_eq!(observer.history.len(), 3, "each event is collected once");
    for sequence in 5..5 + mod_api::MAX_OBSERVED_COMBAT_EVENTS as u64 {
        // UI delivery drains independently of the retained combat history.
        drop(authority.take_committed_ui());
        status(&mut authority, sequence, ActorStatusKind::TotemActivate);
    }
    observer.collect_combat(&authority, &world);
    assert_eq!(observer.history.len(), mod_api::MAX_OBSERVED_COMBAT_EVENTS);
    assert!(
        observer
            .history
            .iter()
            .all(|recorded| recorded.event.sequence > old_sequence && recorded.life == Some(after))
    );
    assert!(observer.history.iter().all(|recorded| recorded.event.local));
}

/// Changing sessions or dimensions discards both event and life evidence without reusing cursors.
#[test]
fn scope_changes_fence_event_lives_and_do_not_replay_old_authority_evidence() {
    let (mut authority, mut world) = fixture();
    let mut observer = ModuleObserver::default();
    observer.bind_scope((7, 0), 0, 0);
    status(&mut authority, 1, ActorStatusKind::TotemActivate);
    observer.collect_combat(&authority, &world);
    let old_sequence = observer.history.back().unwrap().event.sequence;
    for scope in [(8, 0), (8, 1)] {
        observer.bind_scope(
            scope,
            authority.combat_evidence().last().unwrap().revision,
            0,
        );
        assert!(observer.history.is_empty());
        world.session = scope.0;
        world.dimension = scope.1;
        observer.collect_combat(&authority, &world);
        assert!(observer.history.is_empty(), "old events cannot replay");
    }
    world.dimension = 0;
    observer.bind_scope(
        (8, 0),
        authority.combat_evidence().last().unwrap().revision,
        0,
    );
    status(&mut authority, 2, ActorStatusKind::TotemActivate);
    observer.collect_combat(&authority, &world);
    assert_eq!(observer.history.len(), 1);
    assert!(observer.history[0].event.sequence > old_sequence);
    assert_eq!(observer.history[0].event.session, 8);
}
