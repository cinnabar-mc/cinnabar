//! Every frame referenced by an attachable's compiled render controllers, not just standby.

use pack_compile_input::source::read_bounded_source;

use super::invalid;
use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EquipmentBinding,
    EquipmentCategory, EquipmentTexture, MAX_EQUIPMENT_TEXTURE_SIDE,
    MAX_PACK_EQUIPMENT_PIXEL_BYTES,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Cursor, path::Path, sync::Arc};

/// Why one raster was not decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EquipmentRasterRefusal {
    /// A valid raster that does not fit the remaining pixel budget.
    OverBudget,
    /// The source failed to decode, or a side is outside the supported range.
    Undecodable,
}

/// Attachable rasters a compile left out, by identifier, for its caller to count and log.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EquipmentRasterDrops {
    pub over_budget: Vec<Box<str>>,
    pub undecodable: Vec<Box<str>>,
}

impl EquipmentRasterDrops {
    /// Records one refused raster under its reason.
    pub fn record(&mut self, identifier: &str, refusal: EquipmentRasterRefusal) {
        match refusal {
            EquipmentRasterRefusal::OverBudget => self.over_budget.push(identifier.into()),
            EquipmentRasterRefusal::Undecodable => self.undecodable.push(identifier.into()),
        }
    }

    /// Whether `identifier` was already refused.
    #[must_use]
    pub fn contains(&self, identifier: &str) -> bool {
        self.over_budget
            .iter()
            .chain(&self.undecodable)
            .any(|dropped| dropped.as_ref() == identifier)
    }
}

/// Includes the shared enchantment rasters alongside every requested equipment image.
pub(super) fn compile_texture_identifiers_with(
    sources: &[EntityAssetSource],
    identifiers: Vec<&str>,
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_texture_identifiers_with_priorities(
        sources,
        identifiers,
        &[],
        MAX_PACK_EQUIPMENT_PIXEL_BYTES,
        read,
    )
    .map(|(textures, _)| textures)
}

/// Compiles requested rasters within `pixel_budget`, decoding selected identifiers first so they
/// outlast ordinary art when the budget runs out. Reports every raster it refuses.
pub(super) fn compile_texture_identifiers_with_priorities(
    sources: &[EntityAssetSource],
    identifiers: Vec<&str>,
    priorities: &[&str],
    pixel_budget: usize,
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<(Vec<EquipmentTexture>, EquipmentRasterDrops), AssetError> {
    let mut identifiers: Vec<_> = identifiers
        .into_iter()
        .map(str::to_owned)
        .chain(assets::entity::shared_equipment_textures().map(str::to_owned))
        .chain(assets::item::shield::pattern_paths())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::with_capacity(priorities.len() + identifiers.len());
    for &identifier in priorities {
        if seen.insert(identifier.to_owned()) {
            ordered.push(identifier.to_owned());
        }
    }
    identifiers.sort_unstable();
    identifiers.dedup();
    ordered.extend(
        identifiers
            .into_iter()
            .filter(|identifier| seen.insert(identifier.clone())),
    );
    let by_path = sources
        .iter()
        .map(|source| (source.path.as_ref(), source))
        .collect::<HashMap<_, _>>();
    let mut textures = Vec::new();
    let mut drops = EquipmentRasterDrops::default();
    let mut pixel_bytes = 0usize;
    for identifier in ordered {
        let Some(source) = ["png", "tga"].into_iter().find_map(|extension| {
            by_path
                .get(format!("{identifier}.{extension}").as_str())
                .map(|source| (*source, extension))
        }) else {
            continue;
        };
        let (source, extension) = source;
        let bytes = read(source)?;
        let format = if extension == "png" {
            ImageFormat::Png
        } else {
            ImageFormat::Tga
        };
        let (width, height, rgba8) = match decode_raster(&bytes, format, pixel_budget - pixel_bytes)
        {
            Ok(raster) => raster,
            Err(refusal) => {
                drops.record(&identifier, refusal);
                continue;
            }
        };
        pixel_bytes += rgba8.len();
        textures.push(EquipmentTexture {
            identifier: identifier.into(),
            width,
            height,
            rgba8,
        });
    }
    textures.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok((textures, drops))
}

/// Decodes one equipment raster within the per-image and remaining pixel budgets.
pub fn decode_raster(
    bytes: &[u8],
    format: ImageFormat,
    remaining: usize,
) -> Result<(u16, u16, Arc<[u8]>), EquipmentRasterRefusal> {
    use EquipmentRasterRefusal::{OverBudget, Undecodable};
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| Undecodable)?;
    let side = |side: u32| {
        u16::try_from(side)
            .ok()
            .filter(|side| (1..=MAX_EQUIPMENT_TEXTURE_SIDE).contains(side))
            .ok_or(Undecodable)
    };
    let (width, height) = (side(width)?, side(height)?);
    let pixel_bytes = usize::from(width) * usize::from(height) * 4;
    if pixel_bytes > remaining {
        return Err(OverBudget);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EQUIPMENT_TEXTURE_SIDE.into());
    limits.max_image_height = Some(MAX_EQUIPMENT_TEXTURE_SIDE.into());
    limits.max_alloc = Some(MAX_PACK_EQUIPMENT_PIXEL_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| Undecodable)?;
    Ok((width, height, image.into_rgba8().into_raw().into()))
}

pub fn compile_textures_for_assets(
    root: &Path,
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_textures_for_assets_with(assets, bindings, &mut |source| {
        let bytes = read_bounded_source(root, &root.join(source.path.as_ref()))?;
        if bytes.len() != source.source_bytes as usize
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
        {
            return Err(invalid("equipment raster changed after entity compilation"));
        }
        Ok(bytes)
    })
}

/// In-memory counterpart that includes all retained attachable render-frame candidates.
pub fn compile_textures_for_assets_with(
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_textures_for_assets_with_priorities(assets, bindings, &[], read)
        .map(|(textures, _)| textures)
}

/// In-memory equipment raster compilation that decodes `priorities` and armour art first, so they
/// outlast other art if the pack's pixel budget runs out. Reports every raster it refuses.
pub fn compile_textures_for_assets_with_priorities(
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
    priorities: &[&str],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<(Vec<EquipmentTexture>, EquipmentRasterDrops), AssetError> {
    let mut identifiers = bindings
        .iter()
        .map(|binding| binding.texture.identifier.as_ref())
        .collect::<Vec<_>>();
    identifiers.extend(
        bindings
            .iter()
            .flat_map(|binding| binding.resources.textures.values().map(AsRef::as_ref)),
    );
    for layer in &assets.render.layers {
        let rig = assets
            .rig_bindings
            .get(layer.rig as usize)
            .ok_or_else(|| invalid("equipment render rig is absent"))?;
        let symbol = assets
            .symbols
            .get(rig.entity_symbol as usize)
            .ok_or_else(|| invalid("equipment render symbol is absent"))?;
        if symbol.kind != EntityAssetKind::Attachable {
            continue;
        }
        let slots = assets
            .render
            .slots
            .get(layer.first_slot as usize..)
            .and_then(|slots| slots.get(..usize::from(layer.slot_count)))
            .ok_or_else(|| invalid("equipment render slot range is invalid"))?;
        for slot in slots {
            let candidates = assets
                .render
                .candidates
                .get(slot.first_candidate as usize..)
                .and_then(|candidates| candidates.get(..usize::from(slot.candidate_count)))
                .ok_or_else(|| invalid("equipment render candidate range is invalid"))?;
            for candidate in candidates {
                let source = assets
                    .sources
                    .get(candidate.source as usize)
                    .ok_or_else(|| invalid("equipment render texture source is absent"))?;
                if let Some(stem) = source
                    .path
                    .strip_suffix(".png")
                    .or_else(|| source.path.strip_suffix(".tga"))
                {
                    identifiers.push(stem);
                }
            }
        }
    }
    let mut priority_storage = priorities.to_vec();
    priority_storage.extend(
        bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.category,
                    EquipmentCategory::Armor { .. } | EquipmentCategory::ArmorAndHeld { .. }
                )
            })
            .flat_map(|binding| {
                std::iter::once(binding.texture.identifier.as_ref())
                    .chain(binding.resources.textures.values().map(Box::as_ref))
            }),
    );
    compile_texture_identifiers_with_priorities(
        &assets.sources,
        identifiers,
        &priority_storage,
        MAX_PACK_EQUIPMENT_PIXEL_BYTES,
        read,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba([37, 59, 83, 255]))
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    /// One source per raster, named `textures/items/<index>.png`.
    fn sources(count: usize, raster: &[u8]) -> Vec<EntityAssetSource> {
        (0..count)
            .map(|index| EntityAssetSource {
                path: format!("textures/items/{index:04}.png").into(),
                source_bytes: raster.len() as u32,
                source_sha256: [0; 32],
            })
            .collect()
    }

    #[test]
    fn equipment_texture_decoder_refuses_an_image_past_remaining_pixel_budget() {
        let image = png(2, 2);
        assert_eq!(
            decode_raster(&image, ImageFormat::Png, 15).unwrap_err(),
            EquipmentRasterRefusal::OverBudget
        );
        let (_, _, pixels) = decode_raster(&image, ImageFormat::Png, 16).unwrap();
        assert_eq!(pixels.len(), 16);
        assert!(decode_raster(&png(1, 1), ImageFormat::Png, 15).is_ok());
        assert_eq!(
            decode_raster(b"not an image", ImageFormat::Png, 16).unwrap_err(),
            EquipmentRasterRefusal::Undecodable
        );
    }

    #[test]
    fn every_requested_texture_compiles_past_the_startup_carrier_count() {
        let raster = png(1, 1);
        let sources = sources(assets::MAX_EQUIPMENT_TEXTURES + 1, &raster);
        let identifiers = sources
            .iter()
            .map(|source| source.path.trim_end_matches(".png"))
            .collect::<Vec<_>>();
        let (textures, drops) = compile_texture_identifiers_with_priorities(
            &sources,
            identifiers.clone(),
            &[],
            MAX_PACK_EQUIPMENT_PIXEL_BYTES,
            &mut |_| Ok(raster.clone()),
        )
        .unwrap();
        for identifier in identifiers {
            assert!(
                textures
                    .iter()
                    .any(|texture| texture.identifier.as_ref() == identifier),
                "{identifier} was left out"
            );
        }
        assert_eq!(drops, EquipmentRasterDrops::default());
    }

    #[test]
    fn a_priority_texture_outlasts_the_pixel_budget_and_every_refusal_is_reported() {
        let raster = png(1, 1);
        let sources = sources(4, &raster);
        let identifiers = sources
            .iter()
            .map(|source| source.path.trim_end_matches(".png"))
            .collect::<Vec<_>>();
        let (textures, drops) = compile_texture_identifiers_with_priorities(
            &sources,
            identifiers,
            &["textures/items/0003"],
            8,
            &mut |source| {
                if source.path.contains("0002") {
                    Ok(b"not an image".to_vec())
                } else {
                    Ok(raster.clone())
                }
            },
        )
        .unwrap();
        let kept = textures
            .iter()
            .map(|texture| texture.identifier.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(kept, ["textures/items/0000", "textures/items/0003"]);
        assert_eq!(
            drops,
            EquipmentRasterDrops {
                over_budget: vec!["textures/items/0001".into()],
                undecodable: vec!["textures/items/0002".into()],
            }
        );
    }
}
