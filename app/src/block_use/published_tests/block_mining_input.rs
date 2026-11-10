//! A mod mining hold uses the ordinary crosshair and held-mining admission.

use super::attack_hit_position_tests::unpublished_attack_fixture;
use super::*;
use crate::interaction_override::ModInteraction;
use client_interaction::survival_mining::SurvivalMiningRuntime;
use protocol::test_support::transactions::observe_transaction;

/// Advances a held-mining frame without a physical attack or use button.
fn frame(world: &mut World, tick: u64) {
    attack_progress_tests::buttons(world, vec![]);
    attack_progress_tests::ticks(world, tick, tick + 1);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
}

#[test]
fn mining_hold_starts_and_releases_without_pressing_attack() {
    let (mut world, mut captured) = unpublished_attack_fixture(-30.0);
    item_use_tests::hold_snowball(&mut world);
    world.insert_resource(ModInteraction {
        block_mining: true,
        ..Default::default()
    });
    frame(&mut world, 102);
    assert!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target()
            .is_some_and(|(position, _)| position == [4, 2, 6])
    );
    assert!(!world.resource::<MeleeRuntime>().press_pending());
    world.resource_mut::<ModInteraction>().block_mining = false;
    frame(&mut world, 103);
    assert_eq!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target(),
        None
    );
    assert!(
        captured.drain().iter().all(|packet| {
            observe_transaction(packet).is_none_or(|tx| tx.actor_use().is_none())
        })
    );
}

#[test]
fn mining_hold_preserves_actor_obstruction_game_mode_and_focus() {
    for (actor_z, mode, focused, menu) in [
        (7.5, PlayerGameMode::Survival, true, false),
        (-30.0, PlayerGameMode::Spectator, true, false),
        (-30.0, PlayerGameMode::Adventure, true, false),
        (-30.0, PlayerGameMode::Survival, false, false),
        (-30.0, PlayerGameMode::Survival, true, true),
    ] {
        let (mut world, mut captured) = unpublished_attack_fixture(actor_z);
        item_use_tests::hold_snowball(&mut world);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(mode);
        let mut query = world.query::<&mut Window>();
        query.single_mut(&mut world).unwrap().focused = focused;
        world
            .resource_mut::<client_state::ScreenFocus>()
            .menu_visible = menu;
        world.insert_resource(ModInteraction {
            block_mining: true,
            ..Default::default()
        });
        frame(&mut world, 102);
        assert_eq!(
            world
                .resource::<SurvivalMiningRuntime>()
                .destroying_target(),
            None,
            "actor {actor_z}, mode {mode:?}, focused {focused}, menu {menu}"
        );
        assert!(captured.drain().iter().all(|packet| {
            observe_transaction(packet).is_none_or(|tx| tx.actor_use().is_none())
        }));
    }
}

#[test]
fn mining_hold_does_not_interrupt_item_use() {
    let (mut world, _) = unpublished_attack_fixture(-30.0);
    attack_while_using_tests::start_eating(&mut world);
    world.insert_resource(ModInteraction {
        block_mining: true,
        ..Default::default()
    });
    frame(&mut world, 102);
    assert_eq!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target(),
        None
    );
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
}

#[test]
fn mining_hold_preserves_block_reach_with_an_actor_reach_override() {
    let (mut world, _) = unpublished_attack_fixture(-30.0);
    item_use_tests::hold_snowball(&mut world);
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let block = stream
            .collision_store()
            .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
            .unwrap()
            .runtime_id(0, 4, 2, 6)
            .unwrap();
        stream.predict_block([4, 2, 6], 0, stream.air_block_id());
        stream.predict_block([4, 2, 1], 0, block);
    }
    world.insert_resource(ModInteraction {
        block_mining: true,
        attack_reach: Some(mod_api::MAX_ENTITY_REACH_BLOCKS),
        ..Default::default()
    });
    frame(&mut world, 102);
    assert_eq!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target(),
        None
    );
}

#[test]
fn directed_mining_requires_physical_attack_and_preserves_the_presented_ray() {
    for clear_view in [false, true] {
        let (mut world, _captured) = unpublished_attack_fixture(-30.0);
        item_use_tests::hold_snowball(&mut world);
        let stone = assets::pinned_block_registry_bytes();
        let stone =
            assets::read_registry_for_protocol(stone, assets::active_content_registry_protocol())
                .unwrap();
        let air = stone
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:air")
            .unwrap()
            .sequential_id;
        let stone = stone
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:stone")
            .unwrap()
            .sequential_id;
        {
            let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            let sequence = stream.committed_sequence() + 1;
            let mut updates = vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: [5, 2, 6],
                layer: 0,
                network_id: stone,
            }];
            if clear_view {
                updates.push(protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    layer: 0,
                    network_id: air,
                });
            }
            stream
                .submit(sequence, protocol::WorldEvent::BlockUpdates(updates))
                .unwrap();
            test_time::eventually("directed mining terrain", || {
                stream.poll([4.5, 2.620_01, 8.5], 0);
                stream.committed_sequence() >= sequence
            });
        }
        let origin = world.resource::<InteractionOriginSnapshot>().clone();
        let view = *world.resource::<LocalViewPose>();
        world.insert_resource(ModInteraction {
            block_mining_target: Some([5.5, 2.5, 6.5]),
            ..Default::default()
        });
        attack_progress_tests::buttons(&mut world, vec![1]);
        attack_progress_tests::ticks(&mut world, 102, 103);
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        world
            .run_system_cached(crate::melee::produce_attacks)
            .unwrap();
        assert_eq!(
            world
                .resource::<SurvivalMiningRuntime>()
                .destroying_target()
                .map(|(p, _)| p),
            Some([5, 2, 6])
        );
        assert_eq!(*world.resource::<InteractionOriginSnapshot>(), origin);
        assert_eq!(
            world.resource::<LocalViewPose>().eye_translation(),
            view.eye_translation()
        );
        assert_eq!(
            world.resource::<LocalViewPose>().rotation(),
            view.rotation()
        );
        frame(&mut world, 103);
        assert!(
            world
                .resource::<SurvivalMiningRuntime>()
                .destroying_target()
                .is_none()
        );
    }
}

#[test]
fn directed_mining_does_not_mine_an_obstruction_instead_of_its_requested_block() {
    let (mut world, _captured) = unpublished_attack_fixture(-30.0);
    item_use_tests::hold_snowball(&mut world);
    world.insert_resource(ModInteraction {
        block_mining_target: Some([4.5, 2.5, 5.5]),
        ..Default::default()
    });
    attack_progress_tests::buttons(&mut world, vec![1]);
    attack_progress_tests::ticks(&mut world, 102, 103);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    assert!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target()
            .is_none()
    );
}

#[test]
fn directed_mining_requests_do_not_press_attack() {
    let (mut world, _captured) = unpublished_attack_fixture(-30.0);
    item_use_tests::hold_snowball(&mut world);
    world.insert_resource(ModInteraction {
        block_mining_target: Some([4.5, 2.5, 6.5]),
        ..Default::default()
    });
    frame(&mut world, 102);
    assert!(
        world
            .resource::<SurvivalMiningRuntime>()
            .destroying_target()
            .is_none()
    );
    assert!(!world.resource::<MeleeRuntime>().press_pending());
}

#[test]
fn directed_mining_preserves_actor_and_item_directed_attack_ownership() {
    for (actor_z, piercing) in [(7.5, false), (-30.0, true)] {
        let (mut world, _captured) = unpublished_attack_fixture(actor_z);
        item_use_tests::hold_snowball(&mut world);
        if piercing {
            item_use_tests::hold_air_item(&mut world, "fixture:piercing", 1, [].into());
            world
                .resource_mut::<crate::runtime::world::ClientWorld>()
                .stream
                .as_mut()
                .unwrap()
                .set_item_attack_timings(std::sync::Arc::new(
                    [(
                        Box::<str>::from("fixture:piercing"),
                        protocol::ItemAttackTiming {
                            piercing_weapon: true,
                            ..Default::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                ));
        }
        world.insert_resource(ModInteraction {
            block_mining_target: Some([4.5, 2.5, 6.5]),
            ..Default::default()
        });
        attack_progress_tests::buttons(&mut world, vec![1]);
        attack_progress_tests::ticks(&mut world, 102, 103);
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        world
            .run_system_cached(crate::melee::produce_attacks)
            .unwrap();
        assert!(
            world
                .resource::<SurvivalMiningRuntime>()
                .destroying_target()
                .is_none()
        );
    }
}

#[test]
fn explicit_break_batch_sends_once_and_leaves_terrain_to_the_server() {
    let (mut world, mut captured) = unpublished_attack_fixture(-30.0);
    item_use_tests::hold_snowball(&mut world);
    let cells = [[4, 2, 6], [5, 2, 6]];
    let scope = {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let stone = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap()
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
        stream.predict_block([5, 2, 6], 0, stone);
        (
            stream.authority().actor_session_id(),
            stream.current_dimension(),
        )
    };
    world.insert_resource(ModInteraction {
        block_breaks: cells.to_vec(),
        block_break_scope: Some(scope),
        ..Default::default()
    });
    let is_air = |world: &World, cell: [i32; 3]| {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let registries = world.resource::<PhysicsCollisionRegistries>();
        sim::PaletteWorld::new(
            stream.collision_store(),
            registries.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        )
        .is_air(cell)
        .unwrap()
    };
    frame(&mut world, 102);
    assert!(world.resource::<ModInteraction>().block_breaks.is_empty());
    assert!(cells.iter().all(|&cell| !is_air(&world, cell)));
    frame(&mut world, 103);
    riptide_tests::flush_and_acknowledge(&mut world);
    let sent: Vec<_> = captured
        .drain()
        .iter()
        .flat_map(protocol::test_support::packets::predicted_block_breaks)
        .collect();
    assert_eq!(sent, cells);
    // An accepted break arrives as a server update; the unanswered cell stays solid.
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let air = stream.air_block_id();
        let sequence = stream.committed_sequence() + 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: cells[0],
                    layer: 0,
                    network_id: air,
                }]),
            )
            .unwrap();
        test_time::eventually("server break", || {
            stream.poll([4.5, 2.620_01, 8.5], 0);
            stream.committed_sequence() >= sequence
        });
    }
    assert!(is_air(&world, cells[0]));
    assert!(!is_air(&world, cells[1]));
}

#[test]
fn explicit_break_batch_discards_stale_scope_focus_loss_and_denied_modes() {
    for kind in 0..3 {
        let (mut world, _) = unpublished_attack_fixture(-30.0);
        item_use_tests::hold_snowball(&mut world);
        let stream = world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap();
        let mut scope = (
            stream.authority().actor_session_id(),
            stream.current_dimension(),
        );
        match kind {
            0 => scope.0 += 1,
            1 => {
                let mut query = world.query::<&mut Window>();
                query.single_mut(&mut world).unwrap().focused = false;
            }
            _ => world
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(PlayerGameMode::Spectator),
        }
        world.insert_resource(ModInteraction {
            block_breaks: vec![[4, 2, 6]],
            block_break_scope: Some(scope),
            ..Default::default()
        });
        frame(&mut world, 102);
        assert!(world.resource::<ModInteraction>().block_breaks.is_empty());
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let registries = world.resource::<PhysicsCollisionRegistries>();
        let view = sim::PaletteWorld::new(
            stream.collision_store(),
            registries.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        );
        assert!(!view.is_air([4, 2, 6]).unwrap());
    }
}
