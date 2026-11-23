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

/// Samples one actor's visibility, through the visibility reader or the full layer sampler.
fn visibility(
    store: &ActorAnimationStore,
    actor: &TestActor,
    alpha: f32,
    visibility_only: bool,
) -> RenderVisibility {
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    ActorRenderFrame::new(store, &actors, [0.0; 2], actor.position, alpha)
        .into_layers_reusing(actor.runtime_id, false, visibility_only, &mut Vec::new())
        .unwrap()
        .visibility
}

/// A loop-free gate on `query` whose true branch charges about 9,000 ops, so two passing gates
/// together exceed one actor's frame budget.
fn heavy_gate(
    compiled: &mut assets::CompiledEntityAssets,
    query: u32,
    threshold: f32,
    compare: MolangOp,
) -> u32 {
    use MolangOp::*;
    let scalar = crate::test_support::scalar;
    let mut ops = vec![LoadQuery(query), Push(scalar(threshold)), compare];
    let otherwise = 4 + 2 * 4_500 + 2;
    ops.push(JumpIfFalse(otherwise));
    for _ in 0..4_500 {
        ops.extend([Push(scalar(1.0)), Pop]);
    }
    ops.extend([Push(scalar(1.0)), Return, Push(scalar(0.0))]);
    expression(compiled, ops)
}

#[test]
fn memoized_gates_keep_the_budget_fallback_the_expression_cache_decides() {
    let mut compiled = fixture(false);
    let metadata = heavy_gate(&mut compiled, 4, 41.0, MolangOp::GreaterEqual);
    let frame = heavy_gate(&mut compiled, 1, 0.5, MolangOp::Greater);
    compiled.render.layers[0].condition = Some(metadata);
    compiled.render.layers[1].condition = Some(frame);
    compiled.rig_bindings[0].hide_held_items = None;
    // Identical stores, one reusing gate results, both keeping the expression cache.
    let run = |memo: bool| {
        let (mut store, mut actor) = store(compiled.clone());
        mark(&mut actor, 0);
        advance(&mut store, &actor, true);
        let mut sample = |value, alpha, visibility_only| {
            mark(&mut actor, value);
            let sample = || visibility(&store, &actor, alpha, visibility_only);
            if memo {
                sample()
            } else {
                crate::render_frame::without_memo(sample)
            }
        };
        [
            // Each gate passes in turn, so the cached first gate lets the second finish.
            sample(41, 0.25, true),
            sample(41, 0.75, true),
            // Full sampling replaces both cached gate results with failing ones.
            sample(0, 0.25, false),
            // Both gates must now run, exhausting the budget into the tick's policy.
            sample(41, 0.75, true),
        ]
    };
    let reference = run(false);
    assert!(!reference[1].hides_body(), "the shape reaches both gates");
    assert!(reference[3].hides_body(), "the shape exhausts the budget");
    assert_eq!(run(true), reference);
}

/// The metadata layer gate plus a held-item gate on `variable.is_sneaking`, a tick variable the
/// engine writes from the actor's flags.
fn sneaking_gate_fixture() -> assets::CompiledEntityAssets {
    use MolangOp::*;
    let mut compiled = engine_gate_fixture(false);
    let mut symbols = compiled.molang_symbols.into_vec();
    let sneaking = symbols.len() as u32;
    symbols.push(assets::MolangSymbol {
        kind: assets::MolangSymbolKind::Variable,
        identifier: "variable.is_sneaking".into(),
    });
    compiled.molang_symbols = symbols.into();
    compiled.rig_bindings[0].hide_held_items =
        Some(expression(&mut compiled, vec![LoadVariable(sneaking), Not]));
    compiled
}

#[test]
fn memoized_gates_miss_when_a_tick_variable_they_read_changes() {
    let (mut store, mut actor) = store(sneaking_gate_fixture());
    mark(&mut actor, 41);
    // (sneaking at the preceding tick, whether that tick changed the variable)
    for (step, (sneaking, changed)) in [
        (false, true),
        (false, false),
        (true, true),
        (true, false),
        (false, true),
    ]
    .into_iter()
    .enumerate()
    {
        actor.metadata.insert(
            protocol_ids::ActorDataKey::Flags.wire_value(),
            ActorMetadataValue::Flags(
                u64::from(sneaking) << protocol_ids::ActorFlag::Sneaking.wire_value(),
            ),
        );
        advance(&mut store, &actor, true);
        crate::render_frame::take_memo_hits();
        let (cached, fresh, full) = samples(&store, &actor, 0.75);
        assert_eq!(cached, fresh, "step {step}");
        assert_eq!(cached, full, "step {step}");
        assert_eq!(cached.hides_held_items(), !sneaking, "step {step}");
        // Queries stay fixed, so only the variable decides whether the result is reused.
        let hits = crate::render_frame::take_memo_hits();
        assert_eq!(hits, usize::from(step > 0 && !changed), "step {step}");
    }
}
