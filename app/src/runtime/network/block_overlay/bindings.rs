//! Cinnabar material bindings for custom blocks: each matching binding replaces the render method
//! of the block's matching material instances with the closest one its material lowers to.

use protocol::{CustomBlock, CustomStateValue, CustomVisualComponents};
use protocol_ids::BlockRenderMethod;
use resource_pack::LayeredPackView;
use serde_json::Value;

use super::OverlayGaps;

#[derive(Default)]
pub(super) struct MaterialBindings {
    bindings: Vec<Binding>,
}

struct Binding {
    block: Box<str>,
    states: serde_json::Map<String, Value>,
    instance: Option<Box<str>>,
    method: BlockRenderMethod,
}

impl MaterialBindings {
    /// Loads the stack's block bindings; one block passes cannot draw exactly counts as an
    /// approximated material.
    pub(super) fn new(view: &LayeredPackView, gaps: &mut OverlayGaps) -> Self {
        use pack_runtime::entity_pack::collect::{material_bindings_document, material_documents};
        let Some(document) = material_bindings_document(view) else {
            return Self::default();
        };
        let bindings =
            pack_entity_compiler::block_material_bindings(&material_documents(view), &document)
                .into_iter()
                .map(|binding| {
                    let (method, exact) = assets::block_render_method_for(
                        &binding.definition.render_state(),
                        binding.shading.alpha_test,
                    );
                    if !exact {
                        gaps.approximated_materials += 1;
                    }
                    Binding {
                        block: binding.block,
                        states: binding.states,
                        instance: binding.material_instance,
                        method,
                    }
                })
                .collect();
        Self { bindings }
    }

    /// Applies the bindings matching `block` in the state with `values`, in document order.
    pub(super) fn apply(
        &self,
        block: &CustomBlock,
        values: Option<&[CustomStateValue]>,
        components: &mut CustomVisualComponents,
    ) {
        let Some(materials) = components.materials.as_mut() else {
            return;
        };
        for binding in self.bindings.iter().filter(|binding| {
            binding.block.as_ref() == block.name.as_ref() && states_match(binding, block, values)
        }) {
            for instance in materials.iter_mut().filter(|instance| {
                binding.instance.as_deref().is_none_or(|pattern| {
                    assets::entity_render_pattern_matches(pattern, &instance.name)
                })
            }) {
                instance.render_method = binding.method;
            }
        }
    }
}

/// Whether the state has every value the binding names; unknown state names never match.
fn states_match(
    binding: &Binding,
    block: &CustomBlock,
    values: Option<&[CustomStateValue]>,
) -> bool {
    binding.states.iter().all(|(name, expected)| {
        let Some(axis) = block
            .visual
            .state_axes
            .iter()
            .position(|axis| axis.name.as_ref() == name)
        else {
            return false;
        };
        match (values.and_then(|values| values.get(axis)), expected) {
            (Some(CustomStateValue::String(value)), Value::String(expected)) => {
                value.as_ref() == expected
            }
            (Some(CustomStateValue::Int(value)), Value::Number(expected)) => {
                expected.as_i64() == Some(*value)
            }
            (Some(CustomStateValue::Bool(value)), Value::Bool(expected)) => value == expected,
            _ => false,
        }
    })
}
