//! Native ocelot movement state supplied to the pack's animation controller.

use crate::{AnimatedActor, EngineSlots, evaluation::variables::MolangVariables, query};
use protocol_ids::ActorFlag;
use std::sync::OnceLock;

/// Publishes the native base's selector before authored pre-animation scripts run.
pub(super) fn publish(
    engine: &EngineSlots,
    variables: &mut MolangVariables,
    actor: &dyn AnimatedActor,
) {
    let Some(slot) = engine.ocelot_state else {
        return;
    };
    static CLASS: OnceLock<u16> = OnceLock::new();
    let class = *CLASS.get_or_init(|| {
        assets::facts::entity::EntityFacts::pinned()
            .get("minecraft:ocelot")
            .class_key
    });
    if class == 0 || actor.entity_fact().class_key != class {
        return;
    }
    let state = if query::actor_flag(actor, ActorFlag::Sneaking.wire_value()) {
        0.0
    } else if query::actor_flag(actor, ActorFlag::Sprinting.wire_value()) {
        1.0
    } else if query::actor_flag(actor, ActorFlag::Sitting.wire_value()) {
        2.0
    } else {
        3.0
    };
    variables.set(Some(slot), state);
}

#[cfg(test)]
mod tests {
    use crate::{
        ActorTickContext, ActorTickInput, MotionState, VariableLayout,
        evaluation::variables::MolangVariables, test_actor::TestActor, tick,
    };
    use assets::{MolangSymbol, MolangSymbolKind};
    use protocol::{ActorKind, ActorMetadataValue};
    use protocol_ids::{ActorDataKey, ActorFlag};

    fn actor(identifier: &str, flags: u64) -> TestActor {
        let mut actor = TestActor::default();
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        actor.metadata.insert(
            ActorDataKey::Flags.wire_value(),
            ActorMetadataValue::Flags(flags),
        );
        actor
    }

    fn publish(actor: &TestActor, variables: &mut MolangVariables) {
        let layout = VariableLayout::from_symbols(&[MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.state".into(),
        }]);
        tick::apply_engine_variables(
            &layout.engine,
            variables,
            actor,
            &ActorTickContext::default(),
            &ActorTickInput::default(),
            &MotionState::default(),
        );
    }

    #[test]
    fn ocelot_movement_state_tracks_flags_and_native_precedence() {
        let mut variables = MolangVariables::slots(1);
        for mask in 0..8 {
            let sneaking = mask & 1 != 0;
            let sprinting = mask & 2 != 0;
            let sitting = mask & 4 != 0;
            let flags = [
                (sneaking, ActorFlag::Sneaking),
                (sprinting, ActorFlag::Sprinting),
                (sitting, ActorFlag::Sitting),
            ]
            .into_iter()
            .fold(0, |flags, (set, flag)| {
                flags | (u64::from(set) << flag.wire_value())
            });
            publish(&actor("minecraft:ocelot", flags), &mut variables);
            let expected = if sneaking {
                0.0
            } else if sprinting {
                1.0
            } else if sitting {
                2.0
            } else {
                3.0
            };
            assert_eq!(variables.number_at(0), Some(expected), "flags {mask}");
        }
    }

    #[test]
    fn ocelot_state_uses_the_native_base_and_leaves_other_actors_untouched() {
        let mut variables = MolangVariables::slots(1);
        variables.set(Some(0), 91.0);
        for identifier in ["minecraft:cat", "minecraft:parrot", "example:ocelot"] {
            publish(
                &actor(identifier, 1 << ActorFlag::Sitting.wire_value()),
                &mut variables,
            );
            assert_eq!(variables.number_at(0), Some(91.0));
        }
        let mut custom = actor("example:watch_cat", 1 << ActorFlag::Sitting.wire_value());
        custom.entity_fact_override =
            Some(assets::facts::entity::EntityFacts::pinned().get("minecraft:ocelot"));
        publish(&custom, &mut variables);
        assert_eq!(variables.number_at(0), Some(2.0));
    }
}
