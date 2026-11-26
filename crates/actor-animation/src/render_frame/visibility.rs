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
        scratch.visibility = Some(Plan {
            mode: plan(assets, evaluator.layout, state, frame).unwrap_or(Mode::Full),
            clips: frame.motion.clips.iter().map(|clip| clip.clip).collect(),
            controllers: state
                .controllers
                .iter()
                .map(|state| state.controller)
                .collect(),
        });
    }
    let mode = match scratch.visibility.as_ref()?.mode {
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
    let mut variables = scratch.variables.lease(&frame.motion.variables);
    match mode {
        Mode::Variables => {
            variables
                .without_expression_cache(|variables| {
                    sampling::prepare_variables(evaluator, state, swing, variables, budget)
                })
                .ok()?;
        }
        Mode::Gates { .. } => {
            sampling::prepare_engine_variables(evaluator, state, swing, &mut variables)
        }
        Mode::Queries | Mode::Full => {}
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
    Some(RenderVisibility {
        controllers: if layers.is_empty() {
            ControllerActivity::NoControllers
        } else if active {
            ControllerActivity::Active
        } else {
            ControllerActivity::Inactive
        },
        hide_held_items,
    })
}
