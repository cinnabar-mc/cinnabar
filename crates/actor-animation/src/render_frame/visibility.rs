//! Visibility readers replay shared variable setup without sampling unused native poses.
use super::*;

/// Gates with downstream variable writers retain the full sampler's ordering.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Queries,
    /// Gate variables come only from the tick baseline and engine writes, so the rig's setup
    /// scripts cannot change a gate and need not be replayed. Setup and gates are loop-free,
    /// so together they charge at most `reserve` ops; a sample with less budget left replays
    /// setup, since the full sampler could exhaust its budget and keep the fallback.
    Gates {
        reserve: usize,
    },
    Variables,
    Full,
}

/// The immutable topology can be reused while animation and controller identities match.
#[derive(Debug)]
pub(super) struct Plan {
    mode: Mode,
    clips: Vec<usize>,
    controllers: Vec<usize>,
    /// Whether a gate reads the swing or spear variables the engine writes before gates run.
    engine_reads: bool,
    /// The gates' reads when the gates are a pure function of them; `None` disables [`Memo`].
    gate_reads: Option<cache::Inputs>,
    /// The most ops the gates can charge together; `None` when that cannot be bounded.
    gate_ops: Option<usize>,
    /// Layer conditions, then the held-item gate.
    gates: Vec<u32>,
    /// Whether no gate reads a frame, camera or swing input, directly or through a variable.
    frame_independent: bool,
    memo: Option<Memo>,
}

/// The last gate result with the exact values its gates read. Gates that read only queries and
/// tick-baseline variables give the same result for the same values, so an unchanged read set
/// reuses the result instead of evaluating the gates again. Full sampling shares the gates'
/// expression results, so reuse also reinstalls the results that read left behind.
#[derive(Debug)]
struct Memo {
    inputs: cache::Inputs,
    result: Option<(RenderVisibility, evaluation::frame_cache::ExpressionSlots)>,
}

impl Plan {
    /// Checks identities without allocation; weights and clocks cannot change authored writes.
    fn matches(&self, state: &ActorRigState, frame: &FrameState) -> bool {
        self.clips
            .iter()
            .copied()
            .eq(frame.motion.clips.iter().map(|clip| clip.clip))
            && self
                .controllers
                .iter()
                .copied()
                .eq(state.controllers.iter().map(|state| state.controller))
    }
}

/// Finds gates independent of the scripts that visibility-only sampling skips.
fn plan(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &ActorRigState,
    frame: &FrameState,
) -> Option<Mode> {
    let rig = assets.rig_bindings().get(state.rig_binding)?;
    // Empty material choices need the full evaluation to distinguish draws from fallbacks.
    if assets.render_layers(state.rig_binding).iter().any(|layer| {
        assets
            .render_data()
            .material_nodes(layer)
            .any(|node| matches!(node, assets::EntityRenderMaterialSelection::Empty))
    }) {
        return None;
    }
    let mut variables = std::collections::BTreeSet::new();
    for expression in assets
        .render_layers(state.rig_binding)
        .iter()
        .filter_map(|layer| layer.condition)
        .chain(rig.hide_held_items)
    {
        if !ops(assets, expression)?.iter().all(|op| match op {
            MolangOp::LoadVariable(symbol) => {
                variables.insert(*symbol);
                true
            }
            MolangOp::Push(_)
            | MolangOp::PushString(_)
            | MolangOp::LoadThis
            | MolangOp::LoadQuery(_)
            | MolangOp::CallQuery(_)
            | MolangOp::Pop
            | MolangOp::Negate
            | MolangOp::Not
            | MolangOp::Truthy
            | MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Equal
            | MolangOp::NotEqual
            | MolangOp::Jump(_)
            | MolangOp::JumpIfFalse(_)
            | MolangOp::JumpIfTrue(_)
            | MolangOp::Return => true,
            MolangOp::Call(function) => !function.is_random(),
            _ => false,
        }) {
            return None;
        }
    }
    if variables.is_empty() {
        return Some(Mode::Queries);
    }
    let clips: Vec<_> = frame.motion.clips.iter().map(|clip| clip.clip).collect();
    let mut skipped = sampling::animation_expressions(
        assets,
        state.rig_binding,
        state.geometry_binding,
        &state.controllers,
        &clips,
    );
    skipped.extend(rig.scale_expressions.into_iter().flatten());
    skipped.extend(sampling::render_output_expressions(
        assets,
        state.rig_binding,
    ));
    if !independent(assets, &variables, skipped)? {
        return None;
    }
    // Setup's only other gate writers are the engine's swing, spear and item-rotation writes;
    // the rotation factor copies a value setup computes, so a gate reading it needs setup.
    let rotation_gate = layout
        .engine
        .first_person_item_rotation_factor
        .is_some_and(|slot| {
            variables
                .iter()
                .any(|&symbol| layout.variable_slot(symbol) == Some(slot))
        });
    let setup = || rig.held_item_scale.into_iter().chain(rig.pre_animation);
    let gates = assets
        .render_layers(state.rig_binding)
        .iter()
        .filter_map(|layer| layer.condition)
        .chain(rig.hide_held_items);
    let reserve = worst_case_ops(assets, setup())
        .zip(worst_case_ops(assets, gates))
        .map(|(setup, gates)| setup + gates);
    Some(match reserve {
        Some(reserve) if !rotation_gate && independent(assets, &variables, setup())? => {
            Mode::Gates { reserve }
        }
        _ => Mode::Variables,
    })
}

/// The most ops `expressions` can charge when every branch only jumps forward and none
/// loops, so each op runs at most once; `None` when that cannot be shown.
fn worst_case_ops(
    assets: &RuntimeEntityAssets,
    expressions: impl IntoIterator<Item = u32>,
) -> Option<usize> {
    let mut total = 0;
    for expression in expressions {
        let ops = ops(assets, expression)?;
        for (pc, op) in ops.iter().enumerate() {
            let target = match op {
                MolangOp::Jump(target)
                | MolangOp::JumpIfFalse(target)
                | MolangOp::JumpIfTrue(target)
                | MolangOp::Arrow(target)
                | MolangOp::Coalesce(assets::MolangBranch { target, .. }) => *target,
                MolangOp::LoopStart(_)
                | MolangOp::LoopNext(_)
                | MolangOp::LoopBreak(_)
                | MolangOp::ForEachStart(_)
                | MolangOp::ForEachNext(_) => return None,
                _ => continue,
            };
            if usize::from(target) <= pc {
                return None;
            }
        }
        total += ops.len();
    }
    Some(total)
}

/// Whether `expressions` cannot write any of `variables`; `None` for invalid bytecode.
fn independent(
    assets: &RuntimeEntityAssets,
    variables: &std::collections::BTreeSet<u32>,
    expressions: impl IntoIterator<Item = u32>,
) -> Option<bool> {
    for expression in expressions {
        for op in ops(assets, expression)? {
            match op {
                MolangOp::StoreVariable(symbol) if variables.contains(symbol) => {
                    return Some(false);
                }
                // Dynamic actor/iteration contexts cannot prove independence of gate variables.
                MolangOp::Arrow(_) | MolangOp::ForEachStart(_) | MolangOp::ForEachNext(_) => {
                    return Some(false);
                }
                _ => {}
            }
        }
    }
    Some(true)
}

/// Returns a validated expression slice without allocating a per-frame program copy.
fn ops(assets: &RuntimeEntityAssets, expression: u32) -> Option<&[MolangOp]> {
    let expression = assets.molang_expressions().get(expression as usize)?;
    let start = expression.first_op as usize;
    assets
        .molang_ops()
        .get(start..start + usize::from(expression.op_count))
}

#[cfg(test)]
thread_local! { static MEMO_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
thread_local! { static MEMO_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// Production samples always may reuse gate results; tests can run the same path without it.
fn memo_enabled() -> bool {
    #[cfg(test)]
    {
        !MEMO_OFF.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

/// Runs `sample` with gate-result reuse disabled on this thread, keeping every other cache.
#[cfg(test)]
pub(crate) fn without_memo<T>(sample: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            MEMO_OFF.with(|off| off.set(self.0));
        }
    }
    let _restore = Restore(MEMO_OFF.with(|off| off.replace(true)));
    sample()
}

/// Takes this test thread's count of gate results reused from [`Memo`].
#[cfg(test)]
pub(crate) fn take_memo_hits() -> usize {
    MEMO_HITS.with(|hits| hits.replace(0))
}

/// Whether a gate reads the attack time or a completed spear variable, the only variables the
/// engine writes over the tick baseline before gates run; unreadable bytecode counts as a read.
fn reads_engine_writes(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    gates: &[u32],
) -> bool {
    gates.iter().any(|&expression| {
        ops(assets, expression).is_none_or(|ops| {
            ops.iter().any(|op| {
                matches!(op, MolangOp::LoadVariable(symbol)
                if layout.variable_slot(*symbol).is_some_and(|slot| {
                    layout.engine.attack_time == Some(slot) || layout.engine.spear.writes(slot)
                }))
            })
        })
    })
}

/// The gates' read set when every variable they load is a persistent tick-baseline slot and
/// their queries are pure, so equal read values give an equal result.
fn gate_reads(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    gates: &[u32],
) -> Option<cache::Inputs> {
    for &expression in gates {
        for op in ops(assets, expression)? {
            if let MolangOp::LoadVariable(symbol) = op
                && layout.variable_slot(*symbol).is_none()
            {
                return None;
            }
        }
    }
    let inputs = cache::Inputs::new(assets, gates.iter().copied());
    inputs.pure().then_some(inputs)
}

/// Whether the rig's sampled visibility may differ between positive-fraction samples of the same
/// tick state, for instance with the frame fraction or camera. A rig that never samples its
/// frame draws its completed tick visibility. Otherwise unknown counts as differing: only a
/// retained plan whose gates run on the tick baseline without reading frame, camera or swing
/// inputs proves otherwise.
pub(super) fn reads_frame(state: &ActorRigState, actor: &dyn AnimatedActor) -> bool {
    let Some(frame) = state.render_frame.as_ref() else {
        return false;
    };
    // A still swing equal to the tick's attack time samples nothing at any fraction.
    let swing_still = !state.samples_swing_poses
        || state
            .local_swing
            .map(|progress| progress.bedrock)
            .or_else(|| state.remote_swing())
            .is_none_or(|[previous, current]| {
                previous == current && current == frame.motion.input.attack_time
            });
    // The sampler's own early return: such a rig always draws its completed tick layers.
    if !state.samples_render_frames
        && swing_still
        && !frame.needs_swell_sampling(actor)
        && !frame.needs_rotation_sampling()
        && !frame.needs_walk_sampling()
        && !frame.samples_local_walk(state)
    {
        return false;
    }
    if !frame.motion.server_effects.is_empty() {
        return true;
    }
    let scratch = frame.cache.lock().unwrap();
    scratch.visibility.as_ref().is_none_or(|plan| {
        !plan.frame_independent
            || !matches!(plan.mode, Mode::Queries | Mode::Gates { .. })
            || !plan.matches(state, frame)
    })
}

/// Evaluates independent gates using the same prepared variables as the full sampler.
pub(super) fn sample(
    evaluator: &Evaluator<'_>,
    state: &ActorRigState,
    swing: Option<f32>,
    budget: &mut EvalBudget<'_>,
) -> Option<RenderVisibility> {
    let assets = evaluator.assets;
    let rig = assets.rig_bindings().get(state.rig_binding)?;
    let layers = assets.render_layers(state.rig_binding);
    let frame = state.render_frame.as_ref()?;
    let mut scratch = frame.cache.lock().unwrap();
    if scratch
        .visibility
        .as_ref()
        .is_none_or(|plan| !plan.matches(state, frame))
    {
        let gates: Vec<u32> = layers
            .iter()
            .filter_map(|layer| layer.condition)
            .chain(rig.hide_held_items)
            .collect();
        let engine_reads = reads_engine_writes(assets, evaluator.layout, &gates);
        scratch.visibility = Some(Plan {
            mode: plan(assets, evaluator.layout, state, frame).unwrap_or(Mode::Full),
            clips: frame.motion.clips.iter().map(|clip| clip.clip).collect(),
            controllers: state
                .controllers
                .iter()
                .map(|state| state.controller)
                .collect(),
            engine_reads,
            gate_reads: if engine_reads {
                None
            } else {
                gate_reads(assets, evaluator.layout, &gates)
            },
            gate_ops: worst_case_ops(assets, gates.iter().copied()),
            frame_independent: !engine_reads
                && gates.iter().all(|&gate| {
                    evaluator.layout.expression_inputs(gate as usize)
                        == evaluation::PresentationReads::default()
                }),
            gates,
            memo: None,
        });
    }
    let plan = scratch.visibility.as_mut()?;
    let mode = match plan.mode {
        Mode::Gates { reserve } if budget.actor_left < reserve || *budget.world_left < reserve => {
            Mode::Variables
        }
        mode => mode,
    };
    if matches!(mode, Mode::Full)
        || (matches!(mode, Mode::Variables | Mode::Gates { .. })
            && !frame.motion.server_effects.is_empty())
    {
        return None;
    }
    let engine_reads = plan.engine_reads;
    // Only these modes run the gates on the unmodified tick baseline the memo reads. A result is
    // reused only when evaluating the gates would certainly finish within this sample's budget;
    // otherwise exhaustion, which depends on the expression cache, decides the fallback.
    let admitted = plan
        .gate_ops
        .is_some_and(|ops| budget.actor_left >= ops && *budget.world_left >= ops);
    let mut memo = match mode {
        Mode::Queries | Mode::Gates { .. } if cache::enabled() && memo_enabled() && admitted => {
            plan.memo.take().or_else(|| {
                plan.gate_reads.as_ref().map(|reads| Memo {
                    inputs: reads.fresh(),
                    result: None,
                })
            })
        }
        _ => None,
    };
    let unchanged = memo.as_mut().is_some_and(|memo| {
        memo.inputs
            .update(evaluator, &frame.motion.variables, budget)
    });
    // A read would leave the gates' expression results as the memo's own read did.
    let reused = memo.as_ref().filter(|_| unchanged).and_then(|memo| {
        let (visibility, slots) = memo.result.as_ref()?;
        scratch.variables.expressions_mut()?.restore(slots);
        Some(*visibility)
    });
    if let Some(visibility) = reused {
        scratch.visibility.as_mut()?.memo = memo;
        #[cfg(test)]
        MEMO_HITS.with(|hits| hits.set(hits.get() + 1));
        return Some(visibility);
    }
    let mut variables = scratch.variables.lease(&frame.motion.variables);
    match mode {
        Mode::Variables => {
            variables
                .without_expression_cache(|variables| {
                    sampling::prepare_variables(evaluator, state, swing, variables, budget)
                })
                .ok()?;
        }
        // The lease is dropped after the gates, so writes no gate reads change nothing.
        Mode::Gates { .. } if engine_reads => {
            sampling::prepare_engine_variables(evaluator, state, swing, &mut variables)
        }
        Mode::Gates { .. } | Mode::Queries | Mode::Full => {}
    }
    let mut active = false;
    for layer in layers {
        active |= match layer.condition {
            Some(condition) => evaluator
                .run(condition as usize, &mut variables, 0.0, budget)
                .ok()?
                .truthy(),
            None => true,
        };
    }
    let hide_held_items = match rig.hide_held_items {
        Some(expression) => Some(
            evaluator
                .run(expression as usize, &mut variables, 0.0, budget)
                .ok()?
                .truthy(),
        ),
        None => None,
    };
    let visibility = RenderVisibility {
        controllers: if layers.is_empty() {
            ControllerActivity::NoControllers
        } else if active {
            ControllerActivity::Active
        } else {
            ControllerActivity::Inactive
        },
        hide_held_items,
    };
    drop(variables);
    let FrameSamplingCache {
        variables,
        visibility: plan,
        ..
    } = &mut *scratch;
    if let Some(mut memo) = memo
        && let Some(plan) = plan.as_mut()
    {
        let mut slots = memo
            .result
            .take()
            .map(|(_, slots)| slots)
            .unwrap_or_default();
        if variables.expressions().is_some_and(|expressions| {
            expressions.copy_ran(plan.gates.iter().map(|&gate| gate as usize), &mut slots)
        }) {
            memo.result = Some((visibility, slots));
            plan.memo = Some(memo);
        }
    }
    Some(visibility)
}
