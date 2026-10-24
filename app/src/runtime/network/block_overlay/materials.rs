//! Material-instance selection and the compact flags each selected instance produces.

use assets::{
    MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DISABLE_AO, MATERIAL_FLAG_DISABLE_FACE_DIMMING,
    MATERIAL_FLAG_DRY_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_ISOTROPIC, MATERIAL_FLAG_WATER_TINT, Material,
    TextureRef,
};
use pack_compiler::block_geometry::FACE_NAMES;
use protocol::CustomVisualComponents;

use super::{Builder, DIAGNOSTIC_MATERIAL};

impl Builder<'_> {
    /// Resolves a face's material instance: an explicit geometry name, then the
    /// face, then `side` for horizontal faces, then `*`.
    /// Diagnostic fallback keeps the selected instance's sidedness.
    pub(super) fn face_material(
        &mut self,
        components: &CustomVisualComponents,
        face: &str,
        instance: Option<&str>,
        uv_flags: u32,
    ) -> (u32, u32, bool) {
        let Some(chosen) = material_instance(components, face, instance) else {
            return (DIAGNOSTIC_MATERIAL, 0, false);
        };
        let two_sided = chosen.render_method.two_sided();
        let mut flags = assets::block_render_method_flags(chosen.render_method);
        let alpha_flags = flags;
        let tint = tint_flags(chosen.tint_method.as_deref());
        flags |= uv_flags | tint;
        // Alpha-masked tint weights the tint by texture alpha on an opaque face.
        if chosen.alpha_masked_tint() && tint != 0 {
            flags |= assets::MATERIAL_FLAG_OVERLAY_MASK;
        }
        match ao_exponent_flags(chosen.ambient_occlusion) {
            Some(ao) => flags |= ao,
            // Exponents outside the compact hundredths field keep the default exponent.
            None => self.gaps.approximated_materials += 1,
        }
        if !chosen.face_dimming() {
            flags |= MATERIAL_FLAG_DISABLE_FACE_DIMMING;
        }
        if chosen.isotropic() {
            flags |= MATERIAL_FLAG_ISOTROPIC;
        }
        let texture = chosen.texture.to_string();
        // Enabled variations select a weighted positional texture per block, as vanilla terrain.
        if chosen.texture_variations()
            && self
                .catalog
                .choices(&texture, 0)
                .is_some_and(|choices| choices.len() > 1)
        {
            return match self.terrain_material(self.base_keys, &texture, 0, flags) {
                Some(material) => (material, alpha_flags, two_sided),
                None => (DIAGNOSTIC_MATERIAL, 0, two_sided),
            };
        }
        let masks = self.face_masks(&chosen.name);
        if let Some(&material) = self.materials.get(&(texture.clone(), flags, masks)) {
            return (material, alpha_flags, two_sided);
        }
        if !self.can_admit_materials(1) {
            return (DIAGNOSTIC_MATERIAL, 0, two_sided);
        }
        let Some(slot) = self.texture_slot(&texture) else {
            return (DIAGNOSTIC_MATERIAL, 0, two_sided);
        };
        let material = self.overlay.materials.len() as u32;
        self.overlay.materials.push(Material {
            texture: TextureRef::new(slot.page, slot.layer).expect("bounded layer"),
            flags,
            animation: slot.animation,
            glow_mask: masks[0],
            emissive_mask: masks[1],
            mask_strength: masks[2],
            ..assets::Material::unvaried()
        });
        self.materials.insert((texture, flags, masks), material);
        (material, alpha_flags, two_sided)
    }

    /// Raw glow and emissive mask references and strength bits that bindings give material
    /// instance `name` in the current state; an unresolvable mask texture is left out.
    fn face_masks(&mut self, name: &str) -> [u32; 3] {
        let Some(masks) = self.state_masks.for_instance(name).cloned() else {
            return [assets::NO_MASK, assets::NO_MASK, 0];
        };
        let mut reference = |key: Option<&str>| {
            key.and_then(|key| self.texture_slot(key))
                .and_then(|slot| TextureRef::new(slot.page, slot.layer).ok())
                .map_or(assets::NO_MASK, TextureRef::raw)
        };
        [
            reference(masks.glow.as_deref()),
            reference(masks.emissive.as_deref()),
            masks.strength.to_bits(),
        ]
    }
}

/// Encodes an authored AO exponent: 0 disables AO, 1 is the default, and other values use the
/// compact hundredths field. `None` when the field cannot represent the value exactly.
fn ao_exponent_flags(exponent: f32) -> Option<u32> {
    if exponent == 0.0 {
        return Some(MATERIAL_FLAG_DISABLE_AO);
    }
    if exponent == 1.0 {
        return Some(0);
    }
    let encoded = (exponent * assets::MATERIAL_LEAF_AO_EXPONENT_SCALE as f32).round();
    if !(1.0..=assets::MATERIAL_LEAF_AO_EXPONENT_MAX as f32).contains(&encoded) {
        return None;
    }
    let flags = (encoded as u32) << assets::MATERIAL_LEAF_AO_EXPONENT_SHIFT;
    (assets::material_leaf_ao_exponent(flags) == exponent).then_some(flags)
}

/// Resolves a face's material instance: an explicit geometry name, then the face, then `side`
/// for horizontal faces, then `*`.
pub(super) fn material_instance<'c>(
    components: &'c CustomVisualComponents,
    face: &str,
    instance: Option<&str>,
) -> Option<&'c protocol::CustomMaterialInstance> {
    let materials = components.materials.as_deref()?;
    let horizontal = !matches!(face, "up" | "down");
    instance
        .into_iter()
        .chain(std::iter::once(face))
        .chain(horizontal.then_some("side"))
        .chain(std::iter::once("*"))
        .find_map(|name| {
            materials
                .iter()
                .find(|material| material.name.as_ref() == name)
        })
}

/// Whether any face of a full cube resolves to a blending render method.
pub(super) fn cube_blends(components: &CustomVisualComponents) -> bool {
    FACE_NAMES.iter().any(|face| {
        material_instance(components, face, None)
            .is_some_and(|material| material.render_method.blends())
    })
}

/// Biome tint of a material instance's `tint_method`; unknown methods do not tint.
fn tint_flags(method: Option<&str>) -> u32 {
    match method {
        Some("default_foliage") => MATERIAL_FLAG_FOLIAGE_TINT,
        Some("birch_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
        Some("evergreen_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        Some("dry_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE,
        Some("grass") => MATERIAL_FLAG_GRASS_TINT,
        Some("water") => MATERIAL_FLAG_WATER_TINT,
        _ => 0,
    }
}
