//! Compiles startup shield icons with the shared GUI rasterizer.
use crate::entity::{EntityAssetCompilation, compile_equipment_textures};
use assets::gui_item::SHIELD_IDENTIFIER as IDENTIFIER;
use assets::{AssetError, IconSprite};
use std::path::Path;

/// Bakes the admitted startup shield definition from its runtime pack sources.
pub(super) fn compile(
    root: &Path,
    compilation: &EntityAssetCompilation,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(binding) = compilation
        .equipment_bindings
        .iter()
        .find(|binding| binding.identifier.as_ref() == IDENTIFIER)
    else {
        return Ok(None);
    };
    let Some(geometry) = compilation
        .assets
        .geometries
        .iter()
        .find(|geometry| geometry.identifier == binding.geometry.identifier)
    else {
        return Ok(None);
    };
    let textures = compile_equipment_textures(
        root,
        &compilation.assets.sources,
        std::slice::from_ref(binding),
    )?;
    let Some(texture) = textures
        .iter()
        .find(|texture| texture.identifier == binding.texture.identifier)
    else {
        return Ok(None);
    };
    Ok(assets::item::shield::bake_icon(geometry, texture))
}

#[cfg(test)]
mod tests;
