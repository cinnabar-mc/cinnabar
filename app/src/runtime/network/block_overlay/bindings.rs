//! Cinnabar material bindings for custom and vanilla blocks: each matching binding replaces the
//! render method of the block's matching material instances with the closest one its material
//! lowers to.

use protocol::{CustomBlock, CustomStateValue, CustomVisualComponents};
use protocol_ids::BlockRenderMethod;
use resource_pack::LayeredPackView;
use serde_json::Value;

use assets::material_keys::MaterialKeys;

use std::collections::HashMap;

use super::{Builder, DIAGNOSTIC_MATERIAL, OverlayGaps};

#[derive(Default)]
pub(super) struct MaterialBindings {
    bindings: Vec<Binding>,
    /// Whether each binding matched some block state.
    matched: Vec<std::cell::Cell<bool>>,
}

struct Binding {
    block: Box<str>,
    states: serde_json::Map<String, Value>,
    instance: Option<Box<str>>,
    /// Evaluated once per block state, as permutation conditions are.
    condition: Option<pack_entity_compiler::BlockMolang>,
    method: BlockRenderMethod,
    masks: pack_entity_compiler::BlockMaskPaths,
}

impl Binding {
    /// Whether the binding's condition holds where `block_state` reads the state's values; one
    /// that cannot evaluate does not apply.
    fn holds<'a>(
        &self,
        block_state: &dyn Fn(&str) -> Option<pack_entity_compiler::BlockStateValue<'a>>,
    ) -> bool {
        self.condition.as_ref().is_none_or(|condition| {
            condition
                .evaluate(block_state)
                .is_some_and(pack_entity_compiler::BlockMolang::holds)
        })
    }
}

/// Mask textures matching bindings assign to one block state's material instances.
#[derive(Default)]
pub(super) struct StateMasks(Vec<(Option<Box<str>>, pack_entity_compiler::BlockMaskPaths)>);

impl StateMasks {
    /// The masks the last matching binding gives material instance `name`.
    pub(super) fn for_instance(&self, name: &str) -> Option<&pack_entity_compiler::BlockMaskPaths> {
        self.0
            .iter()
            .rev()
            .find(|(pattern, _)| {
                pattern
                    .as_deref()
                    .is_none_or(|pattern| assets::entity_render_pattern_matches(pattern, name))
            })
            .map(|(_, masks)| masks)
    }
}

impl MaterialBindings {
    /// Loads the stack's block bindings, resolving their bases through the material list of
    /// graphics mode `fancy_graphics`; one block passes cannot draw exactly counts as an
    /// approximated material.
    pub(super) fn new(
        view: &LayeredPackView,
        fancy_graphics: bool,
        gaps: &mut OverlayGaps,
    ) -> Self {
        use pack_runtime::entity_pack::collect::{material_bindings_document, material_documents};
        let Some(document) = material_bindings_document(view) else {
            return Self::default();
        };
        let bindings = pack_entity_compiler::block_material_bindings(
            &material_documents(view, fancy_graphics),
            &document,
        )
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
                condition: binding
                    .condition
                    .as_deref()
                    .and_then(pack_entity_compiler::BlockMolang::parse),
                method,
                masks: binding.masks,
            }
        })
        .collect::<Vec<_>>();
        let matched = bindings.iter().map(|_| Default::default()).collect();
        Self { bindings, matched }
    }

    /// Bindings that matched no block state.
    pub(super) fn unmatched(&self) -> u32 {
        self.matched.iter().filter(|matched| !matched.get()).count() as u32
    }

    /// The bindings matching `block` in the state with `values`, recording that they matched.
    fn matching<'a>(
        &'a self,
        block: &'a CustomBlock,
        values: Option<&'a [CustomStateValue]>,
    ) -> impl Iterator<Item = &'a Binding> + 'a {
        self.bindings
            .iter()
            .zip(&self.matched)
            .filter(move |(binding, _)| {
                binding.block.as_ref() == block.name.as_ref()
                    && states_match(binding, block, values)
            })
            .map(|(binding, matched)| {
                matched.set(true);
                binding
            })
    }

    /// The masks of the bindings matching `block` in the state with `values`.
    pub(super) fn state_masks(
        &self,
        block: &CustomBlock,
        values: Option<&[CustomStateValue]>,
    ) -> StateMasks {
        StateMasks(
            self.matching(block, values)
                .filter(|binding| binding.masks.glow.is_some() || binding.masks.emissive.is_some())
                .map(|binding| (binding.instance.clone(), binding.masks.clone()))
                .collect(),
        )
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
        for binding in self.matching(block, values) {
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
    /// The raw texture reference of binding mask `key`, or no mask; a key no terrain texture
    /// resolves is counted once.
    pub(super) fn mask_reference(&mut self, key: Option<&str>) -> u32 {
        let Some(key) = key else {
            return assets::NO_MASK;
        };
        match self
            .texture_slot(key)
            .and_then(|slot| assets::TextureRef::new(slot.page, slot.layer).ok())
        {
            Some(reference) => reference.raw(),
            None => {
                if !self.unresolved_masks.contains(key) {
                    self.unresolved_masks.insert(key.into());
                }
                assets::NO_MASK
            }
        }
    }
}

/// The render method and raw mask words one vanilla binding draws with.
#[derive(Clone, Copy)]
struct VanillaBinding {
    method: BlockRenderMethod,
    masks: [u32; 2],
    strength: u32,
}

impl Builder<'_> {
    /// Rebinds vanilla blocks that bindings name, in the bound states, to copies of their
    /// materials drawn with the binding's render method and masks: full cubes through their
    /// faces, model blocks through a copy of their template span. Tinted cubes keep their base
    /// materials.
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
        // States sharing a template span and binding share one copy.
        let mut copies = HashMap::new();
        for source in keys.block_sources() {
            let Some(record) = records.get(source.visual as usize) else {
                continue;
            };
            // Every matching binding counts as matched; the last one draws.
            let mut selected = None;
            for (index, binding) in bindings.bindings.iter().enumerate() {
                if binding.block.as_ref() == record.name.as_ref()
                    && binding
                        .instance
                        .as_deref()
                        .is_none_or(|pattern| pattern == "*")
                    && vanilla_states_match(binding, record)
                {
                    bindings.matched[index].set(true);
                    selected = Some((index, binding));
                }
            }
            let Some((index, binding)) = selected else {
                continue;
            };
            let masks = [
                binding.masks.glow.as_deref(),
                binding.masks.emissive.as_deref(),
            ]
            .map(|key| self.mask_reference(key));
            let bound = VanillaBinding {
                method: binding.method,
                masks,
                strength: binding.masks.strength.to_bits(),
            };
            let block = base.resolve(assets::NetworkIdMode::Sequential, source.visual);
            if !block.is_known() {
                continue;
            }
            if let Some(template) = block.model_template() {
                let head = match copies.get(&(template, index)) {
                    Some(&head) => head,
                    None => {
                        let head = self.copy_vanilla_model(keys, base, template, bound);
                        copies.insert((template, index), head);
                        head
                    }
                };
                if let Some(head) = head {
                    self.overlay.template_overrides.push((source.visual, head));
                }
                continue;
            }
            if block.kind() != assets::VisualKind::Cube
                || block.variant() != 0
                || block.flags().contains(assets::BlockFlags::LEAF_MODEL)
            {
                continue;
            }
            let mut faces = [None; 6];
            for face in assets::BlockFace::ALL {
                let id = block.face(face).material_id();
                if base.material(id).flags & assets::MATERIAL_FLAG_TINT_MASK != 0 {
                    faces = [None; 6];
                    break;
                }
                faces[face as usize] = self.bound_material(keys, base, id, bound);
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

    /// An overlay copy of base material `id`'s terrain art drawn with `bound`'s render method
    /// and masks, keeping its other flags; `None` when it has no terrain source or is not
    /// admitted.
    fn bound_material(
        &mut self,
        keys: &MaterialKeys,
        base: &assets::RuntimeAssets,
        id: u32,
        bound: VanillaBinding,
    ) -> Option<u32> {
        let texture = keys.material_source(id)?;
        let base = base.material(id);
        let flags = (base.flags
            & !(assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT))
            | assets::block_render_method_flags(bound.method);
        self.terrain_material(
            Some(keys),
            &texture.key,
            texture.variant,
            (flags, base.ao_exponent),
        )
        .map(|id| self.masked_material(id, bound.masks, bound.strength))
    }

    /// Copies the complete template span a vanilla model visual draws from `template`, with the
    /// pot contents before it, into the overlay with bound materials, and returns the copied
    /// head. Quads take the binding's sidedness, as custom-block material instances do. `None`,
    /// copying nothing, when the span is not fixed, or a quad material has no terrain source,
    /// carries carrier-baked tint or is not admitted.
    fn copy_vanilla_model(
        &mut self,
        keys: &MaterialKeys,
        base: &assets::RuntimeAssets,
        template: u32,
        bound: VanillaBinding,
    ) -> Option<u32> {
        let templates = base.model_templates();
        let span = assets::model_template_span(templates, template)?;
        let first = template
            .checked_sub(1)
            .filter(|&previous| {
                templates[previous as usize].flags == assets::MODEL_TEMPLATE_FLAG_POTTED_PLANT
            })
            .unwrap_or(template);
        let copied = &templates[first as usize..(template + span) as usize];
        let quads = |part: &assets::ModelTemplate| {
            &base.model_quads()[part.quad_start as usize..][..part.quad_count as usize]
        };
        if !copied.iter().flat_map(quads).all(|quad| {
            quad.material == DIAGNOSTIC_MATERIAL
                || keys
                    .material_source(quad.material)
                    .is_some_and(|source| source.baked_tint.is_none())
        }) {
            return None;
        }
        let mut materials = HashMap::from([(DIAGNOSTIC_MATERIAL, DIAGNOSTIC_MATERIAL)]);
        for quad in copied.iter().flat_map(quads) {
            if let std::collections::hash_map::Entry::Vacant(entry) = materials.entry(quad.material)
            {
                entry.insert(self.bound_material(keys, base, quad.material, bound)?);
            }
        }
        let sided = if bound.method.two_sided() {
            assets::MODEL_QUAD_FLAG_TWO_SIDED
        } else {
            0
        };
        let head = self.overlay.model_templates.len() as u32 + (template - first);
        for (offset, part) in copied.iter().enumerate() {
            let copy = self.overlay.model_templates.len() as u32;
            if let Some(component) = base.model_random_offset(first + offset as u32) {
                self.overlay.model_random_offsets.push((copy, component));
            }
            self.overlay.model_templates.push(assets::ModelTemplate {
                quad_start: self.overlay.model_quads.len() as u32,
                ..*part
            });
            self.overlay
                .model_quads
                .extend(quads(part).iter().map(|quad| assets::ModelQuad {
                    material: materials[&quad.material],
                    flags: (quad.flags & !assets::MODEL_QUAD_FLAG_TWO_SIDED) | sided,
                    ..*quad
                }));
        }
        Some(head)
    }
}

impl Builder<'_> {
    /// A copy of overlay material `id` with `masks`, or `id` itself without masks. Positional
    /// selectors keep their leaves unmasked.
    fn masked_material(&mut self, id: u32, masks: [u32; 2], strength: u32) -> u32 {
        let material = self.overlay.materials[id as usize];
        if masks == [assets::NO_MASK; 2]
            || material.variation_count != 0
            || !self.can_admit_materials(1)
        {
            return id;
        }
        self.overlay.materials.push(assets::Material {
            glow_mask: masks[0],
            emissive_mask: masks[1],
            mask_strength: strength,
            ..material
        });
        self.overlay.materials.len() as u32 - 1
    }
}

/// Whether a vanilla block state has every value the binding names and its condition holds.
fn vanilla_states_match(binding: &Binding, record: &assets::RegistryRecord) -> bool {
    let block_state = |name: &str| {
        let value = assets::facts::BlockProperty::from_name(name)
            .and_then(|property| record.property(property))?;
        Some(match value {
            assets::facts::StateValue::String(value) => {
                pack_entity_compiler::BlockStateValue::String(value)
            }
            assets::facts::StateValue::Int(value) => {
                pack_entity_compiler::BlockStateValue::Number(*value as f32)
            }
            assets::facts::StateValue::Bool(value) => {
                pack_entity_compiler::BlockStateValue::Number(f32::from(u8::from(*value)))
            }
        })
    };
    binding.holds(&block_state)
        && binding.states.iter().all(|(name, expected)| {
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
                (assets::facts::StateValue::Bool(value), Value::Bool(expected)) => {
                    value == expected
                }
                _ => false,
            }
        })
}

/// Whether the state has every value the binding names and its condition holds; unknown
/// state names never match.
fn states_match(
    binding: &Binding,
    block: &CustomBlock,
    values: Option<&[CustomStateValue]>,
) -> bool {
    let block_state = |name: &str| {
        let axis = block
            .visual
            .state_axes
            .iter()
            .position(|axis| axis.name.as_ref() == name)?;
        Some(match values?.get(axis)? {
            CustomStateValue::String(value) => pack_entity_compiler::BlockStateValue::String(value),
            CustomStateValue::Int(value) => {
                pack_entity_compiler::BlockStateValue::Number(*value as f32)
            }
            CustomStateValue::Bool(value) => {
                pack_entity_compiler::BlockStateValue::Number(f32::from(u8::from(*value)))
            }
        })
    };
    binding.holds(&block_state)
        && binding.states.iter().all(|(name, expected)| {
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
