//! How one declaration's fields apply to a resolving material, under its file's version rules.

use super::*;

impl Resolved {
    /// Applies one declaration's fields with this material's version rules.
    pub(super) fn apply(&mut self, fields: &Map<String, Value>) {
        let modern = self.version >= 1;
        names(
            fields,
            "states",
            modern,
            &mut self.states,
            EntityMaterialState::from_name,
        );
        names(fields, "defines", modern, &mut self.defines, |name| {
            (!name.is_empty() && name.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES).then(|| name.into())
        });
        if self
            .states
            .contains(&EntityMaterialState::EnableAlphaToCoverage)
            && !self.defines.contains("ALPHA_TO_COVERAGE")
        {
            Arc::make_mut(&mut self.defines).insert("ALPHA_TO_COVERAGE".into());
        }
        let definition = &mut self.definition;
        for (key, slot) in [
            ("vertexShader", &mut definition.vertex_shader),
            ("fragmentShader", &mut definition.fragment_shader),
            ("geometryShader", &mut definition.geometry_shader),
        ] {
            if let Some(name) =
                text(fields, key).filter(|name| name.len() <= MAX_ENTITY_MATERIAL_NAME_BYTES)
            {
                *slot = Some(name.into());
            }
        }
        if let Some(value) = text(fields, "depthFunc").and_then(EntityMaterialCompare::from_name) {
            definition.depth_func = value;
        }
        for (key, face) in [
            ("frontFace", &mut definition.front_face),
            ("backFace", &mut definition.back_face),
        ] {
            if let Some(fields) = fields.get(key).and_then(Value::as_object) {
                stencil_face(fields, face);
            }
        }
        if let Some(reference) = fields.get("stencilRef").and_then(Value::as_u64) {
            definition.stencil_ref = Some(reference as u8);
        }
        for (key, slot) in [
            ("stencilReadMask", &mut definition.stencil_read_mask),
            ("stencilWriteMask", &mut definition.stencil_write_mask),
        ] {
            if let Some(mask) = fields.get(key).and_then(Value::as_u64) {
                *slot = mask as u32;
            }
        }
        let factor = |key| text(fields, key).and_then(EntityMaterialBlendFactor::from_name);
        if let Some(source) = factor("blendSrc") {
            definition.blend[0] = source;
        }
        if let Some(destination) = factor("blendDst") {
            definition.blend[1] = destination;
        }
        // An absent alpha source follows the colour source; an absent alpha destination
        // follows only a colour destination authored in the same declaration.
        definition.blend[2] = factor("alphaSrc").unwrap_or(definition.blend[0]);
        if let Some(destination) = factor("alphaDst").or_else(|| factor("blendDst")) {
            definition.blend[3] = destination;
        }
        if let Some(value) = finite_f32(fields, "depthBias") {
            definition.depth_bias = value;
        }
        if let Some(value) = finite_f32(fields, "slopeScaledDepthBias") {
            definition.slope_scaled_depth_bias = value;
        }
        if let Some(value) = text(fields, "msaaSupport").and_then(EntityMaterialMsaa::from_name) {
            definition.msaa = value;
        }
        if let Some(value) =
            text(fields, "primitiveMode").and_then(EntityMaterialPrimitive::from_name)
        {
            definition.primitive = value;
        }
        if let Some(values) = fields.get("vertexFields").and_then(Value::as_array) {
            self.vertex_fields = Some(
                values
                    .iter()
                    .filter_map(|value| value.get("field")?.as_str())
                    .filter_map(EntityMaterialVertexField::from_name)
                    .collect(),
            );
        }
        self.apply_samplers(fields, modern);
        // Nesting, and so this recursion and the table's interning, is bounded only by the
        // JSON parser's recursion limit; parsing must keep that limit enabled.
        self.apply_variants(fields, modern);
    }

    fn apply_samplers(&mut self, fields: &Map<String, Value>, modern: bool) {
        let plain = fields.get("samplerStates").and_then(Value::as_array);
        if modern && plain.is_some() {
            self.samplers = Default::default();
        }
        let entries = plain.or_else(|| fields.get("+samplerStates").and_then(Value::as_array));
        for entry in entries.into_iter().flatten().filter_map(Value::as_object) {
            let Some(slot) = entry
                .get("samplerIndex")
                .and_then(Value::as_u64)
                .filter(|slot| *slot < u64::from(ENTITY_MATERIAL_SAMPLER_SLOTS))
            else {
                continue;
            };
            let stages = match text(entry, "shaderStage") {
                Some("Vertex") => 1,
                Some("Geometry") => 4,
                _ => 2,
            };
            self.samplers[slot as usize] = Some(EntityMaterialSampler {
                slot: slot as u8,
                filter: text(entry, "textureFilter")
                    .and_then(EntityMaterialFilter::from_name)
                    .unwrap_or_default(),
                wrap: text(entry, "textureWrap")
                    .and_then(EntityMaterialWrap::from_name)
                    .unwrap_or_default(),
                stages,
            });
        }
    }

    /// Each variant clones the material parsed so far, then applies its own fields. Version 1
    /// replaces inherited variants with a plain list; otherwise it adds `+variants` and then
    /// removes the inherited or added variants that `-variants` names.
    fn apply_variants(&mut self, fields: &Map<String, Value>, modern: bool) {
        let plain = fields.get("variants").and_then(Value::as_array);
        if modern && plain.is_some() {
            self.variants = Arc::default();
        }
        self.add_variants(fields, plain);
        if modern && plain.is_none() {
            let removed = fields.get("-variants").and_then(Value::as_array);
            for name in removed.into_iter().flatten().filter_map(Value::as_str) {
                if self
                    .variants
                    .iter()
                    .any(|(existing, _)| existing.as_ref() == name)
                {
                    Arc::make_mut(&mut self.variants)
                        .retain(|(existing, _)| existing.as_ref() != name);
                }
            }
        }
    }

    /// Clones and applies each entry of the plain list, or of `+variants` when it is absent,
    /// replacing an existing variant of the same name.
    fn add_variants(&mut self, fields: &Map<String, Value>, plain: Option<&Vec<Value>>) {
        let entries = plain.or_else(|| fields.get("+variants").and_then(Value::as_array));
        for entry in entries.into_iter().flatten().filter_map(Value::as_object) {
            for (name, value) in entry {
                let Some(variant_fields) = value.as_object() else {
                    continue;
                };
                if name.is_empty() || name.len() > MAX_ENTITY_MATERIAL_NAME_BYTES {
                    continue;
                }
                let mut variant = Self {
                    variants: Arc::default(),
                    ..self.clone()
                };
                if let Some(version) = text(variant_fields, "version") {
                    variant.version = u8::from(!version.starts_with('0'));
                }
                variant.apply(variant_fields);
                let variants = Arc::make_mut(&mut self.variants);
                variants.retain(|(existing, _)| existing.as_ref() != name);
                variants.push((name.as_str().into(), variant));
            }
        }
    }
}

/// Whether every present state and define list is an array of strings. A malformed list leaves
/// the material unresolved rather than guessing which entries were meant.
pub(super) fn lists_are_well_formed(fields: &Map<String, Value>) -> bool {
    [
        "states", "+states", "-states", "defines", "+defines", "-defines",
    ]
    .into_iter()
    .filter_map(|key| fields.get(key).filter(|value| !value.is_null()))
    .all(|value| {
        value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string))
    })
}

/// A number that stays finite once narrowed to `f32`; one that overflows is treated as absent,
/// so the inherited value remains instead of an unrepresentable bias.
fn finite_f32(fields: &Map<String, Value>, key: &str) -> Option<f32> {
    fields
        .get(key)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
}

fn text<'a>(fields: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    fields.get(key).and_then(Value::as_str)
}

/// Applies `key`, `+key` and `-key` lists with the version's replace and add rules, copying the
/// shared set only when a list changes it.
fn names<T: Ord + Clone>(
    fields: &Map<String, Value>,
    key: &str,
    modern: bool,
    set: &mut Arc<BTreeSet<T>>,
    parse: impl Fn(&str) -> Option<T>,
) {
    let list = |key: &str| {
        fields.get(key).and_then(Value::as_array).map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter_map(&parse)
                .collect::<Vec<_>>()
        })
    };
    if let Some(values) = list(key) {
        if modern {
            *set = Arc::new(values.into_iter().collect());
        } else if values.iter().any(|value| !set.contains(value)) {
            Arc::make_mut(set).extend(values);
        }
        return;
    }
    let added = list(&format!("+{key}")).unwrap_or_default();
    if added.iter().any(|value| !set.contains(value)) {
        Arc::make_mut(set).extend(added);
    }
    if modern {
        let removed = list(&format!("-{key}")).unwrap_or_default();
        if removed.iter().any(|value| set.contains(value)) {
            let set = Arc::make_mut(set);
            for value in &removed {
                set.remove(value);
            }
        }
    }
}

fn stencil_face(fields: &Map<String, Value>, face: &mut EntityMaterialStencilFace) {
    if let Some(compare) = text(fields, "stencilFunc").and_then(EntityMaterialCompare::from_name) {
        face.compare = compare;
    }
    for (key, slot) in [
        ("stencilFailOp", &mut face.fail),
        ("stencilDepthFailOp", &mut face.depth_fail),
        ("stencilPassOp", &mut face.pass),
    ] {
        if let Some(operation) =
            text(fields, key).and_then(EntityMaterialStencilOperation::from_name)
        {
            *slot = operation;
        }
    }
}
