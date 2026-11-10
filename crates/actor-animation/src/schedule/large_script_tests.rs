//! Large authored scripts complete without bypassing ordered world accounting.
use super::*;
use assets::CompiledMolangExpression;

/// Repeats frame-alpha assignments and poses the final value, optionally initializing too.
fn script_assets(assignments: usize, initialize: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    let mut ops = Vec::new();
    for _ in 0..assignments {
        ops.extend([MolangOp::LoadQuery(1), MolangOp::StoreVariable(7)]);
    }
    ops.push(MolangOp::Push(crate::test_support::scalar(0.0)));
    let script_ops = ops.len();
    let mut expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: u16::try_from(script_ops).unwrap(),
        max_stack: 1,
    }];
    for op in [
        MolangOp::LoadVariable(7),
        MolangOp::LoadVariable(5),
        MolangOp::LoadQuery(1),
    ] {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(op);
    }
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.rig_bindings[0].initialize = initialize.then_some(0);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn large_scripts_publish_each_tick_through_the_expression_storage_bound() {
    for assignments in [3_000, (assets::MAX_MOLANG_OPS_PER_EXPRESSION - 1) / 2] {
        for initialize in [false, true] {
            for pack in [false, true] {
                let assets = script_assets(assignments, initialize);
                let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
                if pack {
                    store.set_pack(Some((assets, vec![0])));
                }
                let actor = TestActor::default();
                store.insert(1, 0, &actor);
                let actors = HashMap::from([(actor.runtime_id, actor)]);
                for tick in 1..=3 {
                    store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
                        frame_alpha: tick as f32,
                        ..Default::default()
                    });
                    let rig = store.get(1).unwrap();
                    assert!(rig.pose_evaluated);
                    assert_eq!(rig.completed_tick, tick);
                    assert_ne!(rig.current, rig.rest);
                    if tick > 1 {
                        assert_ne!(rig.current, rig.previous);
                    }
                }
                let stats = store.stats();
                assert_eq!(stats.actor_budget_exhaustions, 0);
                assert_eq!(stats.world_budget_exhaustions, 0);
            }
        }
    }
}

#[test]
fn large_script_crowds_match_serial_through_world_exhaustion_and_warmups() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    pool.install(|| {
        for pack in [false, true] {
            let assets = script_assets(3_000, true);
            let vanilla = if pack {
                crate::test_support::counting_random_assets()
            } else {
                Arc::clone(&assets)
            };
            let mut serial = ActorAnimationStore::with_assets(Arc::clone(&vanilla));
            serial.schedule.serial = true;
            let mut parallel = ActorAnimationStore::with_assets(vanilla);
            let actors: HashMap<_, _> = (1..=32)
                .map(|id| {
                    (
                        id,
                        TestActor {
                            runtime_id: id,
                            unique_id: id as i64,
                            ..Default::default()
                        },
                    )
                })
                .collect();
            for store in [&mut serial, &mut parallel] {
                if pack {
                    store.set_pack(Some((Arc::clone(&assets), vec![0])));
                }
                for id in 1..=32 {
                    store.insert(1, 0, &actors[&id]);
                }
            }
            let mut completed = std::collections::HashSet::new();
            for tick in 0..8 {
                for store in [&mut serial, &mut parallel] {
                    if tick == 4 {
                        for state in store.rigs.values_mut() {
                            state.culled = true;
                        }
                    }
                    let before = store.stats().evaluated_molang_ops;
                    store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
                        frame_alpha: tick as f32,
                        ..Default::default()
                    });
                    assert!(
                        store.stats().evaluated_molang_ops - before
                            <= MAX_MOLANG_OPS_PER_WORLD_TICK as u64
                    );
                }
                assert_eq!(
                    super::tests::fingerprint(&parallel),
                    super::tests::fingerprint(&serial)
                );
                completed.extend(
                    parallel
                        .rigs
                        .iter()
                        .filter(|(_, state)| state.initialized)
                        .map(|(id, _)| id.runtime_id),
                );
            }
            assert_eq!(completed.len(), actors.len());
            assert!(parallel.stats().world_budget_exhaustions > 0);
            assert_eq!(parallel.stats().actor_budget_exhaustions, 0);
        }
    });
}
