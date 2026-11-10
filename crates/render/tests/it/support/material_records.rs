//! Terrain material table records for GPU fixtures, laid out from the shared record type.

/// Words in one terrain material record, as the material table stores it.
pub const MATERIAL_WORDS: usize = assets::MATERIAL_BYTES / 4;

/// The record words of `material`, in the table's field order. Naming every field makes a new
/// one fail to compile here, and the array length must match [`MATERIAL_WORDS`].
pub fn material_words(material: assets::Material) -> [u32; MATERIAL_WORDS] {
    let assets::Material {
        texture,
        flags,
        animation,
        variation_start,
        variation_count,
        variation_weight,
        glow_mask,
        emissive_mask,
        mask_strength,
        ao_exponent,
    } = material;
    [
        texture.raw(),
        flags,
        animation,
        variation_start,
        variation_count,
        variation_weight,
        glow_mask,
        emissive_mask,
        mask_strength,
        ao_exponent,
    ]
}

/// The record words of an unmasked material drawing raw texture reference `texture` with
/// `flags` and `animation`, without positional variations.
pub fn plain_material(texture: u32, flags: u32, animation: u32) -> [u32; MATERIAL_WORDS] {
    material_words(assets::Material {
        texture: assets::TextureRef::from_raw(texture).expect("a valid texture reference"),
        flags,
        animation,
        ..assets::Material::unvaried()
    })
}
