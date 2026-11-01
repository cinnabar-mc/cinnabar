//! Full-resolution carrier layers for authored models that sample small regions of a large sheet.
//!
//! A carrier layer holds one tile, so a larger sheet's whole-sheet material is a downsample that
//! loses thin art such as fringes. Authored UVs cross any fixed grid and these models already use
//! most of a template's quads, so each distinct region is packed onto tiles instead of splitting
//! quads. Each region owns whole cells of a [`MIP_CELL`] grid, with its edge texels repeated to fill
//! them, so the mip levels that still show a model's shape read only that region's art. The layers
//! are optional, as with [`super::sheet_tiles`]: without room, or with a region wider than a tile,
//! models keep drawing from the downsample.

use super::lily_pad_textures::append_layer;
use super::*;
use assets::{MODEL_TEMPLATE_FLAG_SHEET_TILES, MODEL_UV_SCALE, TILE_SIZE};
use pack_compile_input::image::{DecodedTexture, build_texture_mip_chain};
use std::collections::{BTreeMap, BTreeSet};

const UV_SCALE: u32 = MODEL_UV_SCALE as u32;
/// Regions start and end on this many texels, which isolates them through two mip levels.
const MIP_CELL: u32 = 4;

/// A sheet rectangle in whole texels.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Where one region's top-left texel landed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Slot {
    tile: usize,
    x: u32,
    y: u32,
}

/// The whole texels a quad samples from a square sheet, widened to at least one texel.
fn quad_region(quad: &ModelQuad, side: u32) -> Region {
    let bounds = [0, 1].map(|axis| {
        let values = quad.uvs.map(|uv| u32::from(uv[axis]));
        let low = values.iter().min().map_or(0, |&low| low) * side / UV_SCALE;
        let high = values
            .iter()
            .max()
            .map_or(0, |&high| high)
            .saturating_mul(side)
            .div_ceil(UV_SCALE);
        (low, high.max(low + 1))
    });
    Region {
        x: bounds[0].0,
        y: bounds[1].0,
        width: bounds[0].1 - bounds[0].0,
        height: bounds[1].1 - bounds[1].0,
    }
}

/// The whole cells a region occupies on a tile.
fn footprint(region: Region) -> (u32, u32) {
    (
        region.width.next_multiple_of(MIP_CELL),
        region.height.next_multiple_of(MIP_CELL),
    )
}

/// Packs regions onto tiles in rows, tallest first, so the layout depends only on the regions.
fn pack_regions(regions: &BTreeSet<Region>) -> Option<BTreeMap<Region, Slot>> {
    let mut ordered = regions.iter().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|&region| (std::cmp::Reverse(footprint(region).1), region));
    let mut slots = BTreeMap::new();
    let (mut tile, mut x, mut y, mut row) = (0, 0, 0, 0);
    for region in ordered {
        let (width, height) = footprint(region);
        if width > TILE_SIZE || height > TILE_SIZE {
            return None;
        }
        if x + width > TILE_SIZE {
            (x, y, row) = (0, y + row, 0);
        }
        if y + height > TILE_SIZE {
            (tile, x, y, row) = (tile + 1, 0, 0, 0);
        }
        slots.insert(region, Slot { tile, x, y });
        x += width;
        row = row.max(height);
    }
    Some(slots)
}

/// Copies each packed region onto its tile, repeating its edge texels across the rest of its
/// cells. Texels no region covers stay transparent.
fn tile_pixels(sheet: &DecodedTexture, slots: &BTreeMap<Region, Slot>) -> Vec<Box<[u8]>> {
    let count = slots.values().map(|slot| slot.tile + 1).max().unwrap_or(0);
    let bytes = (TILE_SIZE * TILE_SIZE * 4) as usize;
    let mut tiles = vec![vec![0; bytes].into_boxed_slice(); count];
    for (&region, slot) in slots {
        let (width, height) = footprint(region);
        for row in 0..height {
            for column in 0..width {
                let source_x = region.x + column.min(region.width - 1);
                let source_y = region.y + row.min(region.height - 1);
                let source = ((source_y * sheet.width + source_x) * 4) as usize;
                let target = (((slot.y + row) * TILE_SIZE + slot.x + column) * 4) as usize;
                tiles[slot.tile][target..target + 4]
                    .copy_from_slice(&sheet.rgba8[source..source + 4]);
            }
        }
    }
    tiles
}

/// Maps one sheet coordinate to its tile, keeping any sub-texel authored offset.
fn local_uv(uv: u16, side: u32, start: u32, slot: u32) -> u16 {
    let scale = UV_SCALE / TILE_SIZE;
    (u32::from(uv) * side / TILE_SIZE + slot * scale - start * scale) as u16
}

/// Moves a quad from the whole sheet onto its region's tile.
fn tile_quad(
    quad: &ModelQuad,
    side: u32,
    slots: &BTreeMap<Region, Slot>,
    tiles: &[u32],
) -> ModelQuad {
    let region = quad_region(quad, side);
    let slot = slots[&region];
    let mut tiled = *quad;
    tiled.material = tiles[slot.tile];
    tiled.uvs = quad.uvs.map(|uv| {
        [
            local_uv(uv[0], side, region.x, slot.x),
            local_uv(uv[1], side, region.y, slot.y),
        ]
    });
    tiled
}

/// Whether a drawn template is the first of an equal-shape whole-sheet pair on planned sheets.
fn is_pair(
    index: usize,
    templates: &[ModelTemplate],
    quads: &[ModelQuad],
    art: &BTreeMap<u32, &DecodedTexture>,
) -> bool {
    let (Some(first), Some(next)) = (templates.get(index), templates.get(index + 1)) else {
        return false;
    };
    let span = |part: &ModelTemplate| {
        &quads[part.quad_start as usize..(part.quad_start + part.quad_count) as usize]
    };
    first.flags == 0
        && next.flags == 0
        && first.quad_count == next.quad_count
        && span(first)
            .iter()
            .all(|quad| art.contains_key(&quad.material))
        && span(first)
            .iter()
            .zip(span(next))
            .all(|(first, next)| first == next)
}

/// Appends layers for each planned sheet and moves the first template of its drawn pairs onto
/// them. A sheet without room, or with a region wider than a tile, stays on the whole sheet.
pub(super) fn install(
    art: &BTreeMap<u32, &DecodedTexture>,
    materials: &mut Vec<Material>,
    pages: &mut Vec<TexturePage>,
    visuals: &[BlockVisual],
    templates: &mut [ModelTemplate],
    quads: &mut [ModelQuad],
) -> Result<(), AssetError> {
    let pairs = visuals
        .iter()
        .filter(|visual| visual.kind == VisualKind::Model)
        .map(|visual| visual.model_template as usize)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|&index| is_pair(index, templates, quads, art))
        .collect::<Vec<_>>();
    let mut regions = BTreeMap::<u32, BTreeSet<Region>>::new();
    for &index in &pairs {
        let part = templates[index];
        for quad in &quads[part.quad_start as usize..(part.quad_start + part.quad_count) as usize] {
            regions
                .entry(quad.material)
                .or_default()
                .insert(quad_region(quad, art[&quad.material].width));
        }
    }
    let mut tiled = BTreeMap::new();
    for (sheet, regions) in &regions {
        // Empty UV spans on the sheet's far edge have no texel to copy, so they keep the sheet.
        let side = art[sheet].width;
        if regions
            .iter()
            .any(|region| region.x + region.width > side || region.y + region.height > side)
        {
            continue;
        }
        let Some(slots) = pack_regions(regions) else {
            continue;
        };
        let pixels = tile_pixels(art[sheet], &slots);
        let free = pages
            .iter()
            .map(|page| MAX_CARRIER_TEXTURE_LAYERS.saturating_sub(page.texture.layers as usize))
            .sum::<usize>();
        if free < pixels.len() || materials.len() + pixels.len() > MAX_MATERIALS {
            continue;
        }
        let flags = materials[*sheet as usize].flags;
        let mut tiles = Vec::with_capacity(pixels.len());
        for tile in pixels {
            let texture = append_layer(pages, build_texture_mip_chain(tile)?)?;
            tiles.push(materials.len() as u32);
            materials.push(Material {
                texture,
                flags,
                ..Material::unvaried()
            });
        }
        tiled.insert(*sheet, (slots, tiles));
    }
    for index in pairs {
        let part = &mut templates[index];
        let range = part.quad_start as usize..(part.quad_start + part.quad_count) as usize;
        if !quads[range.clone()]
            .iter()
            .all(|quad| tiled.contains_key(&quad.material))
        {
            continue;
        }
        for quad in &mut quads[range] {
            let (slots, tiles) = &tiled[&quad.material];
            *quad = tile_quad(quad, art[&quad.material].width, slots, tiles);
        }
        part.flags |= MODEL_TEMPLATE_FLAG_SHEET_TILES;
    }
    Ok(())
}

#[cfg(test)]
#[path = "sheet_regions_tests.rs"]
mod tests;
