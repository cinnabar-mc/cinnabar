use super::super::{test_actor::TestActor, tests::actor_with_metadata, tick};
use super::*;
use protocol::{ActorKind, ActorMetadataValue};
use protocol_ids::ActorFlag;
use std::collections::HashMap;

const TRIDENT: &str = "minecraft:trident";

fn engine() -> EngineSlots {
    EngineSlots {
        is_brandishing_spear: Some(0),
        charge_amount: Some(1),
        ..EngineSlots::default()
    }
}

/// A `kind` actor using its main-hand item when `using`, targeting `target` when set.
fn actor(kind: ActorKind, using: bool, target: Option<i64>) -> TestActor {
    let mut metadata = HashMap::from([(
        ActorDataKey::Flags.wire_value(),
        ActorMetadataValue::Flags(u64::from(using) << ActorFlag::UsingItem.wire_value()),
    )]);
    if let Some(target) = target {
        metadata.insert(
            ActorDataKey::Target.wire_value(),
            ActorMetadataValue::Long(target),
        );
    }
    TestActor {
        kind,
        ..actor_with_metadata(metadata)
    }
}

fn player() -> ActorKind {
    ActorKind::Player {
        username: "fixture".into(),
        uuid: Default::default(),
    }
}

fn drowned() -> ActorKind {
    ActorKind::Entity {
        identifier: "minecraft:drowned".into(),
    }
}

/// Holding a trident, whose native use animation is the spear's.
fn holding_trident() -> ActorTickContext {
    let mut context = ActorTickContext {
        main_hand: Some(TRIDENT.into()),
        main_hand_use_animation: Some(ItemUseAnimationFact::Spear),
        main_hand_max_use_ticks: assets::item_use::LONG_WEAPON_USE_TICKS,
        ..ActorTickContext::default()
    };
    context.held_item_facts[0]
        .traits
        .insert(ItemTraits::TRIDENT);
    context
}

/// `[variable.is_brandishing_spear, variable.charge_amount]` after one engine refresh.
fn publish(actor: &TestActor, context: &ActorTickContext, item_use_ticks: u32) -> [f32; 2] {
    let mut variables = MolangVariables::slots(2);
    let input = ActorTickInput {
        item_use_ticks,
        ..ActorTickInput::default()
    };
    tick::apply_engine_variables(
        &engine(),
        &mut variables,
        actor,
        context,
        &input,
        &Default::default(),
    );
    [
        variables.number_at(0).unwrap(),
        variables.number_at(1).unwrap(),
    ]
}

#[test]
fn using_a_trident_brandishes_it_and_charges_over_ten_ticks() {
    let using = actor(player(), true, None);
    let context = holding_trident();
    for (ticks, charge) in [(1, 0.1), (5, 0.5), (10, 1.0), (40, 1.0)] {
        assert_eq!(publish(&using, &context, ticks), [1.0, charge], "{ticks}");
    }
    // The local rig reads its own use clock.
    let local = ActorTickContext {
        local_item_use_ticks: Some(3),
        ..holding_trident()
    };
    assert_eq!(publish(&using, &local, 0), [1.0, 0.3]);
    assert_eq!(
        publish(&actor(player(), false, None), &holding_trident(), 0),
        [0.0, 0.0]
    );
}

#[test]
fn only_the_spear_use_animation_brandishes_a_used_item() {
    let using = actor(player(), true, None);
    for animation in [None, Some(ItemUseAnimationFact::Bow)] {
        let context = ActorTickContext {
            main_hand_use_animation: animation,
            ..holding_trident()
        };
        assert_eq!(publish(&using, &context, 5), [0.0, 0.0], "{animation:?}");
    }
    let empty = ActorTickContext {
        main_hand: None,
        ..holding_trident()
    };
    assert_eq!(publish(&using, &empty, 5), [0.0, 0.0]);
}

#[test]
fn a_mob_holding_a_trident_brandishes_it_at_its_target_but_a_player_does_not() {
    let context = holding_trident();
    for (target, brandishing) in [(None, 0.0), (Some(0), 0.0), (Some(7), 1.0), (Some(-1), 1.0)] {
        assert_eq!(
            publish(&actor(drowned(), false, target), &context, 0),
            [brandishing, 0.0],
            "{target:?}"
        );
    }
    assert_eq!(
        publish(&actor(player(), false, Some(7)), &context, 0),
        [0.0, 0.0]
    );
    let mut sword = holding_trident();
    sword.held_item_facts[0] = Default::default();
    assert_eq!(
        publish(&actor(drowned(), false, Some(7)), &sword, 0),
        [0.0, 0.0]
    );
}
