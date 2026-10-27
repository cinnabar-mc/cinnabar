//! Immutable entity analysis shared by independent actor stores.

use super::{RuntimeEntityAssets, VariableLayout};
use std::sync::Arc;

/// Prepared entity symbols and numeric programs, independent of session actor state.
#[derive(Clone, Debug)]
pub struct ActorAnimationCatalog {
    pub(super) assets: Arc<RuntimeEntityAssets>,
    pub(super) layout: Arc<VariableLayout>,
}

impl ActorAnimationCatalog {
    /// Analyzes this exact carrier on the caller, before its stores enter a frame.
    pub fn new(assets: Arc<RuntimeEntityAssets>) -> Self {
        let layout = Arc::new(VariableLayout::new(&assets));
        Self { assets, layout }
    }

    /// The carrier allocation whose indices the prepared programs use.
    pub fn assets(&self) -> &Arc<RuntimeEntityAssets> {
        &self.assets
    }
}

impl From<Arc<RuntimeEntityAssets>> for ActorAnimationCatalog {
    /// Prepares an unanalysed carrier for callers that do not already own its catalog.
    fn from(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self::new(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActorAnimationStore;

    /// Original expressions make repeated analysis grow independently of actor state.
    fn assets(expressions: usize) -> Arc<RuntimeEntityAssets> {
        let mut compiled = crate::test_support::compiled_fixture();
        let mut programs = compiled.molang_expressions.into_vec();
        let mut ops = compiled.molang_ops.into_vec();
        for _ in 0..expressions {
            programs.push(assets::CompiledMolangExpression {
                first_op: ops.len() as u32,
                op_count: 1,
                max_stack: 1,
            });
            ops.push(assets::MolangOp::Push(crate::test_support::scalar(1.0)));
        }
        compiled.molang_expressions = programs.into_boxed_slice();
        compiled.molang_ops = ops.into_boxed_slice();
        Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
    }

    #[test]
    fn prepared_stores_do_no_catalog_sized_work() {
        for expressions in [16, 2048] {
            let catalog = ActorAnimationCatalog::new(assets(expressions));
            let (store, allocations) =
                test_alloc::measure(|| ActorAnimationStore::with_assets(catalog.clone()));
            assert!(
                allocations.calls <= 64 && allocations.bytes <= 64 * 1024,
                "store repeated preparation for {expressions} expressions: {allocations:?}"
            );
            assert!(Arc::ptr_eq(
                &store.catalog().unwrap().layout,
                &catalog.layout
            ));
        }
    }

    #[test]
    fn shared_catalog_keeps_actor_and_session_state_independent() {
        let catalog = ActorAnimationCatalog::new(assets(16));
        let mut first = ActorAnimationStore::with_assets(catalog.clone());
        first.completed_tick = 17;
        first.set_server_animation_compiler(|_, _| None);
        first.set_pack(Some((Arc::clone(catalog.assets()), vec![0])));
        let second = ActorAnimationStore::with_assets(first.catalog().unwrap());
        assert_eq!(first.completed_tick, 17);
        assert!(first.server_compiler.is_some());
        assert!(first.pack.is_some());
        assert_eq!(second.completed_tick, 0);
        assert!(second.server_compiler.is_none());
        assert!(second.pack.is_none());
        assert!(second.rigs.is_empty());
        assert!(Arc::ptr_eq(&first.layout, &second.layout));
        assert!(Arc::ptr_eq(
            second.catalog().unwrap().assets(),
            catalog.assets()
        ));
    }
}
