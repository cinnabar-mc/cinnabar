//! Opaque terrain depth prepass: Complementary-class packs light terrain in their gbuffers
//! fragment programs, so every overdrawn fragment costs a full lighting evaluation. A
//! depth-only pass first (the pack's own vertex stage and the pack's fragment stage with its
//! colour outputs removed, so only its discards remain) lets the shading pass test against
//! each category's depth and shade each visible pixel once per category. The result is
//! identical when the shading pass neither blends nor has side effects, and both pipelines
//! compute the same position, which [`invariant_position`] guarantees.

use naga::{Binding, BuiltIn, Expression, Handle, Module, ShaderStage, Statement, Type, TypeInner};

#[cfg(test)]
#[path = "depth_prepass_tests.rs"]
mod tests;

/// Whether drawing `terrain` (per output location, the buffer written) over a pixel replaces
/// everything `sky` wrote there, so a sky fragment behind prepassed terrain may be culled.
pub(crate) fn overwrites(terrain: &[Option<u32>], sky: &[Option<u32>]) -> bool {
    sky.iter()
        .flatten()
        .all(|buffer| terrain.contains(&Some(*buffer)))
}

/// Marks the vertex entry point's `gl_Position` invariant, so every pipeline built from the
/// module computes bit-identical depth whatever fragment stage it links with.
pub(crate) fn invariant_position(module: &mut Module) {
    for entry in module
        .entry_points
        .iter_mut()
        .filter(|entry| entry.stage == ShaderStage::Vertex)
    {
        let Some(result) = entry.function.result.as_mut() else {
            continue;
        };
        if let Some(Binding::BuiltIn(BuiltIn::Position { invariant })) = result.binding.as_mut() {
            *invariant = true;
            continue;
        }
        let old = result.ty;
        let Type { name, inner } = module.types[old].clone();
        let TypeInner::Struct { mut members, span } = inner else {
            continue;
        };
        let mut changed = false;
        for member in &mut members {
            if let Some(Binding::BuiltIn(BuiltIn::Position { invariant })) = member.binding.as_mut()
                && !*invariant
            {
                *invariant = true;
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        let new = module.types.insert(
            Type {
                name,
                inner: TypeInner::Struct { members, span },
            },
            naga::Span::UNDEFINED,
        );
        result.ty = new;
        retype(&mut entry.function, old, new);
    }
}

/// Points the entry function's uses of the output struct `old` at `new`.
fn retype(function: &mut naga::Function, old: Handle<Type>, new: Handle<Type>) {
    for (_, expression) in function.expressions.iter_mut() {
        match expression {
            Expression::Compose { ty, .. } | Expression::ZeroValue(ty) if *ty == old => *ty = new,
            _ => {}
        }
    }
    for (_, local) in function.local_variables.iter_mut() {
        if local.ty == old {
            local.ty = new;
        }
    }
}

/// Removes the fragment entry point's outputs, leaving only what decides discards. `None`
/// when the stage writes depth, which a prepass cannot reproduce from the vertex position.
pub(crate) fn depth_only_fragment(module: &Module) -> Option<Module> {
    let mut module = module.clone();
    let entry = module
        .entry_points
        .iter_mut()
        .find(|entry| entry.stage == ShaderStage::Fragment)?;
    if let Some(result) = &entry.function.result {
        let writes_depth = match &module.types[result.ty].inner {
            TypeInner::Struct { members, .. } => members
                .iter()
                .any(|m| matches!(m.binding, Some(Binding::BuiltIn(BuiltIn::FragDepth)))),
            _ => matches!(result.binding, Some(Binding::BuiltIn(BuiltIn::FragDepth))),
        };
        if writes_depth {
            return None;
        }
    }
    entry.function.result = None;
    let body = std::mem::take(&mut entry.function.body);
    entry.function.body = strip_returns(body);
    Some(module)
}

fn strip_returns(block: naga::Block) -> naga::Block {
    let mut out = naga::Block::new();
    for (statement, span) in block.span_into_iter() {
        let statement = match statement {
            Statement::Return { value: Some(_) } => Statement::Return { value: None },
            Statement::Block(inner) => Statement::Block(strip_returns(inner)),
            Statement::If {
                condition,
                accept,
                reject,
            } => Statement::If {
                condition,
                accept: strip_returns(accept),
                reject: strip_returns(reject),
            },
            Statement::Switch { selector, cases } => Statement::Switch {
                selector,
                cases: cases
                    .into_iter()
                    .map(|case| naga::SwitchCase {
                        body: strip_returns(case.body),
                        ..case
                    })
                    .collect(),
            },
            Statement::Loop {
                body,
                continuing,
                break_if,
            } => Statement::Loop {
                body: strip_returns(body),
                continuing,
                break_if,
            },
            other => other,
        };
        out.push(statement, span);
    }
    out
}
