//! Full-resolution carrier layers for art sheets that models sample by region.
//!
//! A carrier layer holds one tile, so a larger sheet's whole-sheet material is a downsample.
//! Region-mapped models also get the sheet's quadrants as separate layers, each normalized like
//! any other tile. The whole-sheet material stays the sheet's identity for texture keys, particles
//! and pack replacements, so nothing else that reads the sheet changes.

use super::lily_pad_textures::append_layer;
use assets::{MAX_CARRIER_TEXTURE_PAGES, TILE_SIZE, TextureMip};
use pack_compile_input::image::build_texture_mip_chain;
use {super::*, assets::BlockFace};

const QUADRANTS: usize = 4;

/// A sheet's quadrant materials, in row order, per whole-sheet material.
pub(super) type SheetTiles = BTreeMap<u32, [u32; QUADRANTS]>;

pub(super) struct Inputs<'a> {
    pub root: &'a Path,
    pub pack: &'a PackSources,
    pub records: &'a [RegistryRecord],
    pub fallback: &'a visuals::fallback::FallbackInventory<'a>,
    pub descriptors: &'a BTreeMap<Descriptor, u32>,
}

/// Appends quadrant layers for each static shelf sheet shared by all six faces.
///
/// A sheet that one tile already holds, an animated or weighted material, per-face art, and a
/// sheet beyond the carrier's budget keep sampling their whole-sheet material.
pub(super) fn install(
    inputs: Inputs<'_>,
    materials: &mut Vec<Material>,
    pages: &mut Vec<TexturePage>,
) -> Result<SheetTiles, AssetError> {
    let Inputs {
        root,
        pack,
        records,
        fallback,
        descriptors,
    } = inputs;
    let mut layers_by_path = BTreeMap::<Box<str>, [TextureRef; QUADRANTS]>::new();
    let mut tiles = SheetTiles::new();
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
        if tiles.contains_key(&sheet)
            || material.animation != NO_ANIMATION
            || material.variation_count != 0
            || materials.len() + QUADRANTS > MAX_MATERIALS
        {
            continue;
        }
        let layers = if let Some(&layers) = layers_by_path.get(&descriptor.path) {
            layers
        } else {
            let Some(chains) = quadrant_chains(root, &descriptor)?.filter(|_| has_room(pages))
            else {
                continue;
            };
            let mut layers = [TextureRef::DIAGNOSTIC; QUADRANTS];
            for (layer, chain) in layers.iter_mut().zip(chains) {
                *layer = append_layer(pages, chain)?;
            }
            layers_by_path.insert(descriptor.path.clone(), layers);
            layers
        };
        let first = materials.len() as u32;
        materials.extend(layers.map(|texture| Material {
            texture,
            flags: material.flags,
            ..Material::unvaried()
        }));
        tiles.insert(sheet, std::array::from_fn(|index| first + index as u32));
    }
    Ok(tiles)
}

/// Whether the carrier's pages can take one more sheet's quadrants.
fn has_room(pages: &[TexturePage]) -> bool {
    let used = pages
        .iter()
        .map(|page| page.texture.layers as usize)
        .sum::<usize>();
    used + QUADRANTS <= MAX_CARRIER_TEXTURE_PAGES * MAX_CARRIER_TEXTURE_LAYERS
}

/// Crops and normalizes a square sheet larger than one tile; `None` for any other image.
fn quadrant_chains(
    root: &Path,
    descriptor: &Descriptor,
) -> Result<Option<[Box<[TextureMip]>; QUADRANTS]>, AssetError> {
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
