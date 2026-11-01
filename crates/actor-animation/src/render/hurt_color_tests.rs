use super::super::*;
use assets::{CompiledMolangExpression, EntityGeometryScalar, MolangOp};

const OVERLAY: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const HURT: [f32; 4] = [0.75, 0.0, 0.0, 0.5];

/// Builds a pure controller with a constant `is_hurt_color`, as Java animation packs author it.
fn hurt_color_assets(identifier: &str) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled(identifier);
    let first = compiled.molang_expressions.len() as u32;
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    for value in [HURT[0], HURT[1], HURT[3]] {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(MolangOp::Push(EntityGeometryScalar::new(value).unwrap()));
    }
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.render.layers[0].color = None;
    compiled.render.layers[0].hurt_color = Some([first, first + 1, first + 1, first + 2]);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// The sampled overlay of the rig's only controller layer.
fn sampled_overlay(store: &mut ActorAnimationStore, actor: &TestActor) -> [f32; 4] {
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    store
        .render_layers(actor, 0.5, [0.0; 2], [0.0; 3], &mut budget, true)
        .unwrap()
        .render[0]
        .overlay
}

/// Native `RenderController::getOverlayColor` only reads `is_hurt_color` while the actor is hurt.
#[test]
fn a_constant_hurt_color_tints_the_actor_only_while_it_is_hurt() {
    let identifier = "custom:hurt_color";
    let mut actor = tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: identifier.into(),
    };
    let mut store = ActorAnimationStore::with_assets(hurt_color_assets(identifier));
    store.insert(1, 0, &actor);
    let advance = |store: &mut ActorAnimationStore, actor: &TestActor| {
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            true,
            |_| Default::default(),
        );
    };

    advance(&mut store, &actor);
    assert_eq!(
        store.get(actor.runtime_id).unwrap().render[0].overlay,
        OVERLAY
    );
    assert_eq!(sampled_overlay(&mut store, &actor), OVERLAY);

    actor.status.overlay_alpha = 0.6;
    advance(&mut store, &actor);
    assert_eq!(store.get(actor.runtime_id).unwrap().render[0].overlay, HURT);
    assert_eq!(sampled_overlay(&mut store, &actor), HURT);

    actor.status.overlay_alpha = 0.0;
    advance(&mut store, &actor);
    assert_eq!(
        store.get(actor.runtime_id).unwrap().render[0].overlay,
        OVERLAY
    );
    assert_eq!(sampled_overlay(&mut store, &actor), OVERLAY);
}

/// A hit starting or ending between ticks reaches the hurt colour without waiting for the next
/// tick, including through samples retained for the current one.
#[test]
fn hurt_colors_follow_damage_between_ticks() {
    let identifier = "custom:hurt_color";
    let mut actor = tests::actor_with_metadata(HashMap::new());
    actor.kind = protocol::ActorKind::Entity {
        identifier: identifier.into(),
    };
    let mut store = ActorAnimationStore::with_assets(hurt_color_assets(identifier));
    store.insert(1, 0, &actor);
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| Default::default(),
    );

    assert_eq!(sampled_overlay(&mut store, &actor), OVERLAY);
    actor.status.overlay_alpha = 0.6;
    assert_eq!(sampled_overlay(&mut store, &actor), HURT);
    actor.status.overlay_alpha = 0.0;
    assert_eq!(sampled_overlay(&mut store, &actor), OVERLAY);
}
