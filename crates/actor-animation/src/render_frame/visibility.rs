//! Visibility readers replay shared variable setup without sampling unused native poses.
use super::*;

/// Gates with downstream variable writers retain the full sampler's ordering.
#[derive(Clone, Copy, Debug)]
enum Mode {
    Queries,
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
fn plan(assets: &RuntimeEntityAssets, state: &ActorRigState, frame: &FrameState) -> Option<Mode> {
    let rig = assets.rig_bindings().get(state.rig_binding)?;
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
    for expression in skipped {
        for op in ops(assets, expression)? {
            match op {
                MolangOp::StoreVariable(symbol) if variables.contains(symbol) => return None,
                // Dynamic actor/iteration contexts cannot prove independence of gate variables.
                MolangOp::Arrow(_) | MolangOp::ForEachStart(_) | MolangOp::ForEachNext(_) => {
                    return None;
                }
                _ => {}
            }
        }
    }
    Some(Mode::Variables)
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
            mode: plan(assets, state, frame).unwrap_or(Mode::Full),
            clips: frame.motion.clips.iter().map(|clip| clip.clip).collect(),
            controllers: state
                .controllers
                .iter()
                .map(|state| state.controller)
                .collect(),
        });
    }
    let mode = scratch.visibility.as_ref()?.mode;
    if matches!(mode, Mode::Full)
        || (matches!(mode, Mode::Variables) && !frame.motion.server_effects.is_empty())
    {
        return None;
    }
    let mut variables = scratch.variables.lease(&frame.motion.variables);
    if matches!(mode, Mode::Variables) {
        variables
            .without_expression_cache(|variables| {
                sampling::prepare_variables(evaluator, state, swing, variables, budget)
            })
            .ok()?;
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
