//! Personal reach overrides pass through the published pick and packet admission path.

use super::attack_hit_position_tests::{attack, unpublished_attack_fixture};
use super::*;
use crate::interaction_override::ModInteraction;
use protocol::test_support::transactions::observe_transaction;

/// Clears the fixture's near obstruction and optionally places one at another position.
fn obstruction(world: &mut World, position: Option<[i32; 3]>) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let block = stream
        .collision_store()
        .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
        .unwrap()
        .runtime_id(0, 4, 2, 6)
        .unwrap();
    stream.predict_block([4, 2, 6], 0, stream.air_block_id());
    if let Some(position) = position {
        stream.predict_block(position, 0, block);
    }
}

#[test]
fn mod_reach_admits_distant_attacks_only_within_the_requested_range() {
    for (reach, z, expected) in [
        (None, 3.0, 0),
        (Some(4.0), 3.0, 0),
        (Some(mod_api::MAX_ENTITY_REACH_BLOCKS), 3.0, 1),
        (Some(mod_api::MAX_ENTITY_REACH_BLOCKS), 2.25, 1),
        (Some(mod_api::MAX_ENTITY_REACH_BLOCKS), 1.5, 0),
    ] {
        let (mut world, mut captured) = unpublished_attack_fixture(z);
        obstruction(&mut world, None);
        world.insert_resource(ModInteraction {
            attack_reach: reach,
            ..Default::default()
        });
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        attack(&mut world);
        let packets = captured.drain();
        let attacks: Vec<_> = packets
            .iter()
            .filter_map(observe_transaction)
            .filter_map(|tx| tx.actor_use().cloned())
            .filter(|tx| tx.action == Some(protocol::ActorUseAction::Attack))
            .collect();
        assert_eq!(attacks.len(), expected, "reach {reach:?}, actor z {z}");
        for actor in attacks {
            assert_eq!(actor.runtime_id, 99);
            assert_eq!(actor.from_position, [4.5, 2.620_01, 8.5]);
            let distance = gameplay::melee::pick_distance([4.5, 2.5, 8.5], actor.hit_position);
            assert!(
                f64::from(distance) > client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH
            );
            assert!(distance < reach.unwrap());
        }
    }
}

#[test]
fn mod_reach_preserves_obstruction_and_spectator_attack_gates() {
    for (blocked, mode) in [
        (true, PlayerGameMode::Survival),
        (false, PlayerGameMode::Spectator),
    ] {
        let (mut world, mut captured) = unpublished_attack_fixture(2.5);
        obstruction(&mut world, blocked.then_some([4, 2, 3]));
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(mode);
        world.insert_resource(ModInteraction {
            attack_reach: Some(mod_api::MAX_ENTITY_REACH_BLOCKS),
            ..Default::default()
        });
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        attack(&mut world);
        assert!(
            captured.drain().iter().all(|packet| {
                observe_transaction(packet).is_none_or(|tx| tx.actor_use().is_none())
            }),
            "blocked {blocked}, mode {mode:?}"
        );
    }
}

#[test]
fn mod_reach_does_not_turn_distant_blocks_into_mining_swings() {
    for reach in [None, Some(mod_api::MAX_ENTITY_REACH_BLOCKS)] {
        let (mut world, mut captured) = unpublished_attack_fixture(-30.0);
        obstruction(&mut world, Some([4, 2, 2]));
        let origin = frame_origin(&world, Vec3::new(4.5, 2.5, 8.8), Quat::IDENTITY);
        world.insert_resource(origin);
        world.insert_resource(ModInteraction {
            attack_reach: reach,
            ..Default::default()
        });
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        attack(&mut world);
        let expected = protocol::test_support::packets::swing(&protocol::swing_arm_packet(
            42,
            protocol::SwingSource::Attack,
        ))
        .unwrap();
        let swings: Vec<_> = captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::packets::swing)
            .collect();
        assert_eq!(swings, [expected], "reach {reach:?}");
        assert_eq!(
            world
                .resource::<crate::melee::MeleeTarget>()
                .attack
                .unwrap()
                .crosshair,
            gameplay::melee::Crosshair::Miss
        );
    }
}
