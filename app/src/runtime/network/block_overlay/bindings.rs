//! Cinnabar material bindings for custom blocks: each matching binding replaces the render method
//! of the block's matching material instances with the closest one its material lowers to.

use protocol::{CustomBlock, CustomStateValue, CustomVisualComponents};
use protocol_ids::BlockRenderMethod;
use resource_pack::LayeredPackView;
use serde_json::Value;

use assets::material_keys::MaterialKeys;

use super::{Builder, OverlayGaps};

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

impl Builder<'_> {
    /// Rebinds the faces of vanilla full cubes that bindings name, in the bound states, to
    /// copies of their materials drawn with the binding's render method. Tinted and model blocks
    /// keep their base materials.
    pub(super) fn bind_vanilla_blocks(&mut self, keys: &MaterialKeys, bindings: &MaterialBindings) {
        let Some(base) = self.base_assets else {
            return;
        };
        if !bindings
            .bindings
            .iter()
            .any(|binding| binding.block.starts_with("minecraft:"))
        {
            return;
        }
        let Ok(records) = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        ) else {
            return;
        };
        for source in keys.block_sources() {
            let Some(record) = records.get(source.visual as usize) else {
                continue;
            };
            let Some(method) = bindings
                .bindings
                .iter()
                .filter(|binding| {
                    binding.block.as_ref() == record.name.as_ref()
                        && binding
                            .instance
                            .as_deref()
                            .is_none_or(|pattern| pattern == "*")
                        && vanilla_states_match(binding, record)
                })
                .last()
                .map(|binding| binding.method)
            else {
                continue;
            };
            let block = base.resolve(assets::NetworkIdMode::Sequential, source.visual);
            if !block.is_known()
                || block.kind() != assets::VisualKind::Cube
                || block.model_template().is_some()
                || block.variant() != 0
                || block.flags().contains(assets::BlockFlags::LEAF_MODEL)
            {
                continue;
            }
            let mut faces = [None; 6];
            for face in assets::BlockFace::ALL {
                let id = block.face(face).material_id();
                let material = base.material(id);
                if material.flags & assets::MATERIAL_FLAG_TINT_MASK != 0 {
                    faces = [None; 6];
                    break;
                }
                let Some(texture) = keys.material_source(id) else {
                    continue;
                };
                let flags = (material.flags
                    & !(assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT))
                    | assets::block_render_method_flags(method);
                faces[face as usize] =
                    self.terrain_material(Some(keys), &texture.key, texture.variant, flags);
            }
            if faces.iter().all(Option::is_none) {
                continue;
            }
            match self
                .overlay
                .face_overrides
                .iter_mut()
                .find(|(visual, _)| *visual == source.visual)
            {
                Some((_, existing)) => {
                    for (face, material) in faces.into_iter().enumerate() {
                        if material.is_some() {
                            existing[face] = material;
                        }
                    }
                }
                None => self.overlay.face_overrides.push((source.visual, faces)),
            }
        }
    }
}

/// Whether a vanilla block state has every value the binding names.
fn vanilla_states_match(binding: &Binding, record: &assets::RegistryRecord) -> bool {
    binding.states.iter().all(|(name, expected)| {
        let Some(value) = assets::facts::BlockProperty::from_name(name)
            .and_then(|property| record.property(property))
        else {
            return false;
        };
        match (value, expected) {
            (assets::facts::StateValue::String(value), Value::String(expected)) => {
                value.as_ref() == expected
            }
            (assets::facts::StateValue::Int(value), Value::Number(expected)) => {
                expected.as_i64() == Some(i64::from(*value))
            }
            (assets::facts::StateValue::Bool(value), Value::Bool(expected)) => value == expected,
            _ => false,
        }
    })
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
