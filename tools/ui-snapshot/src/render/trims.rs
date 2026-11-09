//! Installed armor, masks and palettes exercised through production trim consumers.
use crate::carriers::{self, Error};
use std::{path::Path, sync::Arc};

/// Loads only anonymous installed artwork; the source directory is never copied into the repo.
pub(super) fn png(
    size: [u32; 2],
    dir: &Path,
    source: Option<&Path>,
    trimmed: bool,
) -> Result<Vec<u8>, Error> {
    let source = source.ok_or_else(|| {
        Error::Missing(
            "armor-trims requires --resource-pack with the fetched Bedrock resource_pack directory"
                .into(),
        )
    })?;
    carriers::install_default_skin(dir)?;
    let equipment = assets::RuntimeEquipmentCatalog::decode(&carriers::read(
        &dir.join(assets::carriers::EQUIPMENT.output),
        assets::MAX_EQUIPMENT_CARRIER_BYTES,
    )?)
    .map_err(|e| e.to_string())?;
    let icons = assets::RuntimeIconCatalog::decode(&carriers::read(
        &dir.join(assets::carriers::ICON.output),
        assets::MAX_ICON_CARRIER_BYTES,
    )?)
    .map_err(|e| e.to_string())?;
    let mut textures = equipment.textures().to_vec();
    load_trims(
        source,
        &source.join(assets::item::trim::TEXTURE_DIRECTORY),
        &mut textures,
    )?;
    let catalog = assets::RuntimeEquipmentCatalog::from_parts(
        equipment.source_manifest_sha256(),
        equipment.bindings().to_vec(),
        textures,
    )
    .map_err(|e| e.to_string())?;
    let (mesh, pages, icons) =
        client_ui::ui_runtime::presentation::gui_models::test_support::trim_gallery(
            Arc::new(catalog),
            Arc::new(icons),
            trimmed,
        );
    super::cosmetic_particles::mesh_png(size, mesh, pages, &icons)
}

/// Reads the requested pack's PNG trim sources, including palette overrides absent from old carriers.
fn load_trims(
    root: &Path,
    dir: &Path,
    out: &mut Vec<assets::EquipmentTexture>,
) -> Result<(), Error> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| Error::Missing(format!("{}: {e}", dir.display())))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            load_trims(root, &path, out)?;
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("png") {
            continue;
        }
        let image = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .with_extension("");
        let identifier = relative.to_string_lossy().replace('\\', "/");
        if out
            .iter()
            .any(|texture| texture.identifier.as_ref() == identifier)
        {
            continue;
        }
        out.push(assets::EquipmentTexture {
            identifier: identifier.into(),
            width: image
                .width()
                .try_into()
                .map_err(|e: std::num::TryFromIntError| e.to_string())?,
            height: image
                .height()
                .try_into()
                .map_err(|e: std::num::TryFromIntError| e.to_string())?,
            rgba8: image.into_raw().into(),
        });
    }
    Ok(())
}
