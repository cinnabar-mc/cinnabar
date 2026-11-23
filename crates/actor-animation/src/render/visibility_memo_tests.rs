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

#[test]
fn memoized_gates_leave_the_expression_results_full_sampling_shares() {
    // The held-item gate is costly at metadata 41 and the held-item scale above fraction 0.5;
    // both together exceed one actor's frame budget unless one result is already cached.
    let mut compiled = fixture(false);
    let hide = heavy_gate(&mut compiled, 4, 41.0, MolangOp::GreaterEqual);
    let scale = heavy_gate(&mut compiled, 1, 0.5, MolangOp::Greater);
    compiled.rig_bindings[0].hide_held_items = Some(hide);
    compiled.rig_bindings[0].held_item_scale = Some(scale);
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
            // Warms the held-item gate's expression result and the kept visibility.
            sample(41, 0.75, true),
            // Full sampling replaces that expression result.
            sample(0, 0.25, false),
            // The same gate reads again; reuse must leave the expression cache as a read would.
            sample(41, 0.75, true),
            // Full sampling then finishes only if the held-item gate's result is cached.
            sample(41, 0.75, false),
        ]
    };
    let reference = run(false);
    crate::render_frame::take_memo_hits();
    let reused = run(true);
    let hits = crate::render_frame::take_memo_hits();
    assert_eq!(hits, 1, "only the repeated read reuses its result");
    assert!(
        reference[0].hides_held_items(),
        "the shape reaches the gate"
    );
    assert!(
        reference[3].hides_held_items(),
        "the cached gate lets full sampling finish"
    );
    assert_eq!(reused, reference);
}

#[test]
fn memo_copies_each_distinct_gate_once_however_many_layers_share_it() {
    use MolangOp::*;
    let scalar = crate::test_support::scalar;
    // Every occurrence still fits the tick's own budget, so the tick evaluates visibility.
    const DISTINCT: usize = 500;
    let mut compiled = fixture(false);
    let template = compiled.render.layers[0];
    let (slot, candidate) = (compiled.render.slots[0], compiled.render.candidates[0]);
    let mut layers = Vec::with_capacity(2 * DISTINCT);
    for gate in 0..DISTINCT {
        // Frame-fraction gates miss whenever the fraction changes; each is shared by two layers.
        let condition = expression(
            &mut compiled,
            vec![
                LoadQuery(1),
                Push(scalar(gate as f32 / DISTINCT as f32)),
                Greater,
            ],
        );
        for _ in 0..2 {
            let mut layer = template;
            layer.condition = Some(condition);
            layer.first_slot = layers.len() as u32;
            layers.push(layer);
        }
    }
    // Canonical render data gives every layer its own slot and candidate.
    compiled.render.slots = (0..layers.len())
        .map(|index| {
            let mut slot = slot;
            slot.first_candidate = index as u32;
            slot
        })
        .collect();
    compiled.render.candidates = vec![candidate; layers.len()].into();
    compiled.render.layers = layers.into();
    let (mut store, actor) = store(compiled);
    advance(&mut store, &actor, true);
    for (step, alpha) in [0.25, 0.75, 0.75, 0.5, 0.25].into_iter().enumerate() {
        crate::evaluation::frame_cache::take_copy_work();
        crate::render_frame::take_memo_hits();
        let (cached, fresh, _) = samples(&store, &actor, alpha);
        assert_eq!(cached, fresh, "step {step}");
        let reused = crate::render_frame::take_memo_hits() == 1;
        assert_eq!(reused, step == 2, "step {step}");
        // A miss copies every distinct layer gate and the held-item gate exactly once.
        let expected = if reused { 0 } else { DISTINCT + 1 };
        assert_eq!(
            crate::evaluation::frame_cache::take_copy_work(),
            expected,
            "step {step}"
        );
    }
}

/// Gates whose worst case exceeds one rig's frame budget may fall back to the tick's policy
/// depending on the branches they take and the expression cache, so equal revisions cannot
/// prove the visibility unchanged even when the gates read only tick state.
#[test]
fn over_budget_gates_keep_visibility_frame_dependent() {
    use MolangOp::*;
    let check = |heavy: bool| {
        let mut compiled = fixture(false);
        if heavy {
            let first = heavy_gate(&mut compiled, 4, 41.0, GreaterEqual);
            let second = heavy_gate(&mut compiled, 4, 42.0, GreaterEqual);
            compiled.render.layers[0].condition = Some(first);
            compiled.render.layers[1].condition = Some(second);
        }
        compiled.rig_bindings[0].hide_held_items = None;
        // A tint that reads the frame fraction makes the rig sample every frame.
        let tint = expression(&mut compiled, vec![LoadQuery(1)]);
        compiled.render.layers[0].color = Some([tint; 4]);
        let (mut store, mut actor) = store(compiled);
        // The heavy gates' cheap branches let the tick finish; their expensive ones could not.
        mark(&mut actor, 0);
        advance(&mut store, &actor, true);
        let revision = store.presentation_revision(1);
        let samples = [0.25, 0.75].map(|alpha| visibility(&store, &actor, alpha, true));
        assert_eq!(store.presentation_revision(1), revision);
        (store.visibility_reads_frame(&actor), samples)
    };
    let (light, _) = check(false);
    assert!(
        !light,
        "affordable tick-only gates prove the visibility unchanged"
    );
    let (heavy, _) = check(true);
    assert!(heavy, "gates that can exhaust the budget may fall back");
}
