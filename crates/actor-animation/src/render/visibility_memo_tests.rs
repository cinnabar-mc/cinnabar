use super::super::*;
use super::visibility_tests::{advance, expression, fixture, mark, store, variable_fixture};

/// Samples visibility three ways: the memoized visibility reader, the same reader with every
/// cache disabled, and the full layer sampler.
fn samples(
    store: &ActorAnimationStore,
    actor: &TestActor,
    alpha: f32,
) -> (RenderVisibility, RenderVisibility, RenderVisibility) {
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let camera = [
        actor.position[0],
        actor.position[1],
        actor.position[2] + 9.0,
    ];
    let sample = |visibility_only: bool| {
        ActorRenderFrame::new(store, &actors, [0.0; 2], camera, alpha)
            .into_layers_reusing(actor.runtime_id, false, visibility_only, &mut Vec::new())
            .unwrap()
            .visibility
    };
    let cached = sample(true);
    let fresh = crate::render_frame::cache::uncached(|| sample(true));
    let full = crate::render_frame::cache::uncached(|| sample(false));
    (cached, fresh, full)
}

#[test]
fn memoized_query_gates_match_fresh_evaluation_across_frame_and_actor_changes() {
    let (mut store, mut actor) = store(fixture(true));
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    // (frame fraction, metadata, whether a tick completes first, whether the reads repeat)
    let steps = [
        (0.75, 41, false, false),
        (0.75, 41, false, true),
        (0.25, 41, false, false),
        (0.25, 41, false, true),
        (0.75, 41, false, false),
        // Metadata changes between ticks, which no tick or frame identity records.
        (0.75, 42, false, false),
        (0.75, 42, false, true),
        (0.75, 11, false, false),
        (0.75, 37, true, false),
        (0.75, 37, false, true),
        (0.25, 41, true, false),
    ];
    for (step, (alpha, value, tick, repeated)) in steps.into_iter().enumerate() {
        mark(&mut actor, value);
        if tick {
            advance(&mut store, &actor, true);
        }
        crate::render_frame::take_memo_hits();
        let (cached, fresh, full) = samples(&store, &actor, alpha);
        assert_eq!(cached, fresh, "step {step}");
        assert_eq!(cached, full, "step {step}");
        assert_eq!(
            crate::render_frame::take_memo_hits(),
            usize::from(repeated),
            "step {step}: only unchanged reads reuse the result"
        );
    }
}

/// A layer gate on metadata and, optionally, the engine-written attack time; the held-item gate
/// reads a tick-baseline variable no setup script writes, so the gates need no setup replay.
fn engine_gate_fixture(reads_attack: bool) -> assets::CompiledEntityAssets {
    use MolangOp::*;
    let scalar = crate::test_support::scalar;
    let mut compiled = variable_fixture();
    let pre = expression(
        &mut compiled,
        vec![LoadVariable(8), StoreVariable(7), Push(scalar(0.0))],
    );
    compiled.rig_bindings[0].pre_animation = Some(pre);
    let mut gate = vec![LoadQuery(4), Push(scalar(41.0)), GreaterEqual];
    if reads_attack {
        gate.extend([LoadVariable(5), Push(scalar(0.5)), Greater, Multiply]);
    }
    let gate = expression(&mut compiled, gate);
    for layer in &mut compiled.render.layers {
        layer.condition = Some(gate);
    }
    compiled.rig_bindings[0].hide_held_items =
        Some(expression(&mut compiled, vec![LoadVariable(9), Not]));
    compiled
}

#[test]
fn memoized_baseline_gates_match_fresh_evaluation_and_never_cache_swing_reads() {
    for reads_attack in [false, true] {
        let (mut store, mut actor) = store(engine_gate_fixture(reads_attack));
        store.sync_local_swing(
            actor.runtime_id,
            LocalSwingProgress {
                bedrock: [0.1, 0.9],
                java: [0.1, 0.9],
                frame_alpha: None,
            },
        );
        mark(&mut actor, 41);
        advance(&mut store, &actor, true);
        let mut hits = 0;
        // The swing interpolates 0.1 to 0.9, so the attack gate passes only at 0.75.
        for (step, (alpha, value)) in [(0.25, 41), (0.75, 41), (0.25, 41), (0.25, 7), (0.75, 7)]
            .into_iter()
            .enumerate()
        {
            mark(&mut actor, value);
            crate::render_frame::take_memo_hits();
            let (cached, fresh, full) = samples(&store, &actor, alpha);
            hits += crate::render_frame::take_memo_hits();
            assert_eq!(cached, fresh, "attack {reads_attack}, step {step}");
            assert_eq!(cached, full, "attack {reads_attack}, step {step}");
            let attack = !reads_attack || alpha > 0.5;
            assert_eq!(
                cached.hides_body(),
                !(value >= 41 && attack),
                "attack {reads_attack}, step {step}"
            );
        }
        // Without the attack read, alpha changes nothing the gates read until metadata does.
        assert_eq!(
            hits,
            if reads_attack { 0 } else { 3 },
            "attack {reads_attack}"
        );
    }
}
