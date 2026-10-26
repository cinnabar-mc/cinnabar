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
    /// Binding indices by block identifier, in document order.
    by_block: HashMap<Box<str>, Vec<usize>>,
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
    /// The masks the last binding matching material instance `name` gives it, the one whose
    /// render method it draws with; none when that binding names no mask.
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
            .filter(|masks| masks.glow.is_some() || masks.emissive.is_some())
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
        let mut by_block = HashMap::<Box<str>, Vec<usize>>::new();
        for (index, binding) in bindings.iter().enumerate() {
            by_block
                .entry(binding.block.clone())
                .or_default()
                .push(index);
        }
        Self {
            bindings,
            matched,
            by_block,
        }
    }

    /// The indices of the bindings naming block `name`, in document order.
    fn for_block(&self, name: &str) -> &[usize] {
        self.by_block.get(name).map_or(&[], Vec::as_slice)
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
        self.for_block(&block.name)
            .iter()
            .filter(move |&&index| states_match(&self.bindings[index], block, values))
            .map(|&index| {
                self.matched[index].set(true);
                &self.bindings[index]
            })
    }

    /// Applies the bindings matching `block` in the state with `values`, in document order, and
    /// returns their masks.
    pub(super) fn apply(
        &self,
        block: &CustomBlock,
        values: Option<&[CustomStateValue]>,
        components: &mut CustomVisualComponents,
    ) -> StateMasks {
        let matching: Vec<_> = self.matching(block, values).collect();
        if let Some(materials) = components.materials.as_mut() {
            for binding in &matching {
                for instance in materials.iter_mut().filter(|instance| {
                    binding.instance.as_deref().is_none_or(|pattern| {
                        assets::entity_render_pattern_matches(pattern, &instance.name)
                    })
                }) {
                    instance.render_method = binding.method;
                }
            }
        }
        StateMasks(
            matching
                .into_iter()
                .map(|binding| (binding.instance.clone(), binding.masks.clone()))
                .collect(),
        )
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
            for &index in bindings.for_block(&record.name) {
                let binding = &bindings.bindings[index];
                if binding
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
    /// and masks, keeping its other flags; `None` when it has no terrain source, is not
    /// admitted or cannot draw with the method.
    fn bound_material(
        &mut self,
        keys: &MaterialKeys,
        base: &assets::RuntimeAssets,
        id: u32,
        bound: VanillaBinding,
    ) -> Option<u32> {
        let flags = self.bound_flags(base.material(id).flags, bound.method)?;
        self.material_copy(keys, base, id, flags, bound.masks, bound.strength)
    }

    /// Base material `flags` drawn with `method`, or `None`, counted as approximated, when the
    /// other flags cannot draw with it, as cauldron water cannot be a cutout. A blended method
    /// drops the cutout-only two-sided flag, since blended quads are two-sided themselves.
    fn bound_flags(&mut self, flags: u32, method: BlockRenderMethod) -> Option<u32> {
        let mut cleared = assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT;
        if method.blends() {
            cleared |= assets::MATERIAL_FLAG_TWO_SIDED;
        }
        let flags = (flags & !cleared) | assets::block_render_method_flags(method);
        if !assets::compiled::material_flags_are_valid(flags) {
            self.gaps.approximated_materials += 1;
            return None;
        }
        Some(flags)
    }

    /// An overlay copy of base material `id`'s terrain art drawn with `flags` and `masks`;
    /// `None` when it has no terrain source or is not admitted.
    fn material_copy(
        &mut self,
        keys: &MaterialKeys,
        base: &assets::RuntimeAssets,
        id: u32,
        flags: u32,
        masks: [u32; 2],
        strength: u32,
    ) -> Option<u32> {
        let texture = keys.material_source(id)?;
        self.terrain_material(
            Some(keys),
            &texture.key,
            texture.variant,
            (flags, base.material(id).ao_exponent),
        )
        .map(|id| self.masked_material(id, masks, strength))
    }

    /// Copies the complete template span a vanilla model visual draws from `template`, with the
    /// pot contents before it, into the overlay with bound materials, and returns the copied
    /// head. Quads take the binding's sidedness, as custom-block material instances do; a quad
    /// whose material cannot draw with the binding's method keeps its base material and
    /// sidedness. `None`, copying and admitting nothing, when the span is not fixed, or a quad
    /// material has no terrain source, carries carrier-baked tint or is not admitted.
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
        let sided = if bound.method.two_sided() {
            assets::MODEL_QUAD_FLAG_TWO_SIDED
        } else {
            0
        };
        let checkpoint = self.texture_checkpoint();
        let approximated = self.gaps.approximated_materials;
        // Each base material's copy, and the sidedness its quads take when it is bound.
        let mut materials =
            HashMap::from([(DIAGNOSTIC_MATERIAL, (DIAGNOSTIC_MATERIAL, Some(sided)))]);
        for quad in copied.iter().flat_map(quads) {
            if let std::collections::hash_map::Entry::Vacant(entry) = materials.entry(quad.material)
            {
                let flags = base.material(quad.material).flags;
                let copy = match self.bound_flags(flags, bound.method) {
                    Some(flags) => self
                        .material_copy(
                            keys,
                            base,
                            quad.material,
                            flags,
                            bound.masks,
                            bound.strength,
                        )
                        .map(|id| (id, Some(sided))),
                    None => self
                        .material_copy(keys, base, quad.material, flags, [assets::NO_MASK; 2], 0)
                        .map(|id| (id, None)),
                };
                let Some(copy) = copy else {
                    // Admit the model's materials only together.
                    self.rollback_textures(checkpoint);
                    self.gaps.approximated_materials = approximated;
                    return None;
                };
                entry.insert(copy);
            }
        }
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
                .extend(quads(part).iter().map(|quad| {
                    let (material, sided) = materials[&quad.material];
                    assets::ModelQuad {
                        material,
                        flags: sided.map_or(quad.flags, |sided| {
                            (quad.flags & !assets::MODEL_QUAD_FLAG_TWO_SIDED) | sided
                        }),
                        ..*quad
                    }
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
