//! Visibility-only readers avoid native pose work when gates read queries directly.
use super::*;

/// Independent gates need neither animation channels nor texture selection. Variable reads,
/// writes and random programs retain the full evaluator's script order and budget behavior.
pub(super) fn sample(
    evaluator: &Evaluator<'_>,
    state: &ActorRigState,
    budget: &mut EvalBudget<'_>,
) -> Option<RenderVisibility> {
    let assets = evaluator.assets;
    let rig = assets.rig_bindings().get(state.rig_binding)?;
    let layers = assets.render_layers(state.rig_binding);
    for expression in layers
        .iter()
        .filter_map(|layer| layer.condition)
        .chain(rig.hide_held_items)
    {
        let expression = assets.molang_expressions().get(expression as usize)?;
        let start = expression.first_op as usize;
        if !assets
            .molang_ops()
            .get(start..start + usize::from(expression.op_count))?
            .iter()
            .all(|op| match op {
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
            })
        {
            return None;
        }
    }
    let frame = state.render_frame.as_ref()?;
    let mut scratch = frame.cache.lock().unwrap();
    let mut variables = scratch.variables.lease(&frame.motion.variables);
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
