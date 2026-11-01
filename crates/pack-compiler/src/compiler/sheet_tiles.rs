//! Full-resolution carrier layers for art sheets that models sample by region.
//!
//! A carrier layer holds one tile, so a larger sheet's whole-sheet material is a downsample.
//! Region-mapped models also get the sheet's quadrants as separate layers, each normalized like
//! any other tile. The whole-sheet material stays the sheet's identity for texture keys, particles
//! and pack replacements, so nothing else that reads the sheet changes.
//!
//! The layers are optional. Models are compiled as a pair of whole-sheet templates, the required
//! texture copies come before installation, and installation only fills existing pages. A full
//! carrier keeps every model and pack it could already hold and simply draws these sheets from
//! their downsampled layer.

use super::lily_pad_textures::append_layer;
use assets::{MODEL_TEMPLATE_FLAG_SHEET_TILES, MODEL_TEMPLATE_FLAG_SHELF, TILE_SIZE, TextureMip};
use pack_compile_input::image::build_texture_mip_chain;
use std::collections::BTreeSet;
use {super::*, assets::BlockFace};

const QUADRANTS: usize = 4;
/// Half a tile or sheet in model UV units.
const HALF_UV: u16 = 2048;

type Chains = [Box<[TextureMip]>; QUADRANTS];

pub(super) struct Inputs<'a> {
    pub root: &'a Path,
    pub pack: &'a PackSources,
    pub records: &'a [RegistryRecord],
    pub fallback: &'a visuals::fallback::FallbackInventory<'a>,
    pub descriptors: &'a BTreeMap<Descriptor, u32>,
}

/// Sheets eligible for quadrant layers, by whole-sheet material, and their cropped art.
#[derive(Default)]
pub(in crate::compiler) struct SheetPlan {
    paths: BTreeMap<u32, Box<str>>,
    chains: BTreeMap<Box<str>, Chains>,
}

impl SheetPlan {
    /// Whether templates drawing this material should be compiled as a pair.
    pub(in crate::compiler) fn contains(&self, material: u32) -> bool {
        self.paths.contains_key(&material)
    }

    /// Appends each sheet's layers while the carrier has room, then moves the first template of
    /// each drawn pair onto them. A sheet without room leaves both templates on the whole sheet.
    pub(super) fn install(
        self,
        materials: &mut Vec<Material>,
        pages: &mut Vec<TexturePage>,
        visuals: &[BlockVisual],
        templates: &mut [ModelTemplate],
        quads: &mut [ModelQuad],
    ) -> Result<(), AssetError> {
        let mut layers_by_path = BTreeMap::<&str, [TextureRef; QUADRANTS]>::new();
        let mut tiles = BTreeMap::new();
        for (&sheet, path) in &self.paths {
            let Some(chains) = self.chains.get(path) else {
                continue;
            };
            let known = layers_by_path.get(path.as_ref()).copied();
            if materials.len() + QUADRANTS > MAX_MATERIALS || (known.is_none() && !has_room(pages))
            {
                continue;
            }
            let layers = if let Some(layers) = known {
                layers
            } else {
                let mut layers = [TextureRef::DIAGNOSTIC; QUADRANTS];
                for (layer, chain) in layers.iter_mut().zip(chains) {
                    *layer = append_layer(pages, chain.clone())?;
                }
                layers_by_path.insert(path, layers);
                layers
            };
            let first = materials.len() as u32;
            let flags = materials[sheet as usize].flags;
            materials.extend(layers.map(|texture| Material {
                texture,
                flags,
                ..Material::unvaried()
            }));
            tiles.insert(sheet, std::array::from_fn(|index| first + index as u32));
        }
        let drawn = visuals
            .iter()
            .filter(|visual| visual.kind == VisualKind::Model)
            .map(|visual| visual.model_template)
            .collect::<BTreeSet<_>>();
        for template in drawn {
            let Some(part) = templates.get_mut(template as usize) else {
                continue;
            };
            if part.flags != MODEL_TEMPLATE_FLAG_SHELF {
                continue;
            }
            let range = part.quad_start as usize..(part.quad_start + part.quad_count) as usize;
            let Some(tiled) = tile_quads(&quads[range.clone()], &tiles) else {
                continue;
            };
            quads[range].copy_from_slice(&tiled);
            part.flags |= MODEL_TEMPLATE_FLAG_SHEET_TILES;
        }
        Ok(())
    }
}

/// Finds and crops each static shelf sheet that all six faces share and one tile cannot hold.
/// Animated or weighted materials, per-face art and other images stay on the whole sheet.
pub(super) fn plan(inputs: Inputs<'_>, materials: &[Material]) -> Result<SheetPlan, AssetError> {
    let Inputs {
        root,
        pack,
        records,
        fallback,
        descriptors,
    } = inputs;
    let mut plan = SheetPlan::default();
    for record in records
        .iter()
        .filter(|record| visuals::shelf::is_record(record))
    {
        let mut faces = BlockFace::ALL
            .into_iter()
            .map(|face| descriptor_for(fallback, pack, record, face).map(|(found, _)| found));
        let Some(Some(descriptor)) = faces.next() else {
            continue;
        };
        if faces.any(|face| face.as_ref() != Some(&descriptor)) {
            continue;
        }
        let Some(&sheet) = descriptors.get(&descriptor) else {
            continue;
        };
        let material = materials[sheet as usize];
        if plan.contains(sheet)
            || material.animation != NO_ANIMATION
            || material.variation_count != 0
        {
            continue;
        }
        if !plan.chains.contains_key(&descriptor.path) {
            let Some(chains) = quadrant_chains(root, &descriptor)? else {
                continue;
            };
            plan.chains.insert(descriptor.path.clone(), chains);
        }
        plan.paths.insert(sheet, descriptor.path);
    }
    Ok(plan)
}

/// Whether the existing pages can take one more sheet's quadrants. A new base page would cost
/// session overlays one of their texture arrays, which optional layers must not do.
fn has_room(pages: &[TexturePage]) -> bool {
    let free = pages
        .iter()
        .map(|page| MAX_CARRIER_TEXTURE_LAYERS.saturating_sub(page.texture.layers as usize))
        .sum::<usize>();
    free >= QUADRANTS
}

/// Crops and normalizes a square sheet larger than one tile; `None` for any other image.
fn quadrant_chains(root: &Path, descriptor: &Descriptor) -> Result<Option<Chains>, AssetError> {
    let path = static_texture_path(root, &descriptor.path, &descriptor.texture_key)?;
    let decoded = decode_texture(&path, &descriptor.texture_key)?;
    if decoded.width != decoded.height
        || decoded.width <= TILE_SIZE
        || !decoded.width.is_power_of_two()
    {
        return Ok(None);
    }
    let side = decoded.width as usize;
    let half = side / 2;
    let mut chains = Vec::with_capacity(QUADRANTS);
    for quadrant in 0..QUADRANTS {
        let (left, top) = ((quadrant % 2) * half, (quadrant / 2) * half);
        let mut rgba8 = Vec::with_capacity(half * half * 4);
        for row in top..top + half {
            let start = (row * side + left) * 4;
            rgba8.extend_from_slice(&decoded.rgba8[start..start + half * 4]);
        }
        let tile = normalize_texture_tile(rgba8.into(), half as u32, &descriptor.path)?;
        chains.push(build_texture_mip_chain(tile)?);
    }
    Ok(chains.try_into().ok())
}

/// Moves each whole-sheet quad onto the quadrant layer holding its region at full resolution.
/// Native shelf regions never cross a quadrant; a quad that does, or a sheet without layers,
/// returns `None`.
fn tile_quads(
    sheet: &[ModelQuad],
    tiles: &BTreeMap<u32, [u32; QUADRANTS]>,
) -> Option<Vec<ModelQuad>> {
    sheet
        .iter()
        .map(|quad| {
            let tiles = tiles.get(&quad.material)?;
            let quadrant = [0, 1].map(|axis| {
                let values = quad.uvs.map(|uv| uv[axis]);
                u16::from(values.iter().min().is_some_and(|&low| low >= HALF_UV))
            });
            let mut tiled = *quad;
            for uv in &mut tiled.uvs {
                for axis in 0..2 {
                    uv[axis] = uv[axis]
                        .checked_sub(quadrant[axis] * HALF_UV)
                        .filter(|&local| local <= HALF_UV)?
                        * 2;
                }
            }
            tiled.material = tiles[usize::from(quadrant[0] + 2 * quadrant[1])];
            Some(tiled)
        })
        .collect()
}

#[cfg(test)]
#[path = "sheet_tiles_tests.rs"]
mod tests;
