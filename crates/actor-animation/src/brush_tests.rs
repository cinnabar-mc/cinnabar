use super::super::{test_actor::TestActor, tests::actor_with_metadata, tick};
use super::*;
use protocol::ActorMetadataValue;
use protocol_ids::{ActorDataKey, ActorFlag};
use std::collections::HashMap;

fn engine() -> EngineSlots {
    EngineSlots {
        is_using_brush: Some(0),
        ..EngineSlots::default()
    }
}

/// An actor whose using-item flag is `using`.
fn actor(using: bool) -> TestActor {
    actor_with_metadata(HashMap::from([(
        ActorDataKey::Flags.wire_value(),
        ActorMetadataValue::Flags(u64::from(using) << ActorFlag::UsingItem.wire_value()),
    )]))
}

/// Holding a brush, whose native use animation is the brush's.
fn holding_brush() -> ActorTickContext {
    ActorTickContext {
        main_hand: Some("minecraft:brush".into()),
        main_hand_use_animation: Some(ItemUseAnimationFact::Brush),
        ..ActorTickContext::default()
    }
}

/// `variable.is_using_brush` after one engine refresh.
fn publish(actor: &TestActor, context: &ActorTickContext, item_use_ticks: u32) -> f32 {
    let mut variables = MolangVariables::slots(1);
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
    variables.number_at(0).unwrap()
}

#[test]
fn using_a_brush_publishes_the_brushing_flag() {
    assert_eq!(publish(&actor(true), &holding_brush(), 1), 1.0);
    assert_eq!(publish(&actor(true), &holding_brush(), 300), 1.0);
    // A remote actor whose flag just arrived has not counted a use tick yet.
    assert_eq!(publish(&actor(true), &holding_brush(), 0), 1.0);
    assert_eq!(publish(&actor(false), &holding_brush(), 0), 0.0);
}

#[test]
fn the_local_rig_reads_its_own_use_clock() {
    let local = ActorTickContext {
        local_item_use_ticks: Some(4),
        ..holding_brush()
    };
    assert_eq!(publish(&actor(true), &local, 0), 1.0);
    assert_eq!(publish(&actor(false), &local, 0), 0.0);
}

#[test]
fn only_a_used_main_hand_brush_animation_brushes() {
    for animation in [
        None,
        Some(ItemUseAnimationFact::Spear),
        Some(ItemUseAnimationFact::Spyglass),
    ] {
        let context = ActorTickContext {
            main_hand_use_animation: animation,
            ..holding_brush()
        };
        assert_eq!(publish(&actor(true), &context, 5), 0.0, "{animation:?}");
    }
    let empty = ActorTickContext {
        main_hand: None,
        ..holding_brush()
    };
    assert_eq!(publish(&actor(true), &empty, 5), 0.0);
}
