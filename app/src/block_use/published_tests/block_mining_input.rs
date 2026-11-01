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
