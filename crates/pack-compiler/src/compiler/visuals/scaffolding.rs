//! Scaffolding: four corner poles, a ring of rails under a thin plate, and a second ring
//! at the foot of a block with nothing to stand on.
//!
//! The compound chain starts at that foot ring, so every consumer that follows the chain
//! draws the free-standing model. Meshing starts one part later on a supported block.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::cuboid_quads;
use assets::{MODEL_TEMPLATE_FLAG_SCAFFOLDING, registry::BlockTraits};
use {super::super::*, assets::BlockFace};

const INSET: i16 = 32;
const FAR: i16 = 224;
const FULL: i16 = 256;
const PLATE_LOW: i16 = 255;
/// Texture units for the art's outer band and its opposite edge.
const BAND: u16 = 512;
const BAND_END: u16 = 3584;
const WHOLE: u16 = 4096;

/// Texture coordinates assigned to a face, as `[u0, u1, v0, v1]`.
type Rect = [u16; 4];

/// A compass flip of one pole: which way its art runs across the block.
#[derive(Clone, Copy)]
enum Flip {
    /// Along x on the north side, with its Down art unrotated.
    R0 = 0,
    /// Along z on the east side.
    R1 = 1,
    /// Along z on the west side.
    R2 = 2,
    /// Along x on the south side.
    R3 = 3,
}

/// Identifies scaffolding by its registry trait.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.traits.contains(BlockTraits::SCAFFOLDING)
}

/// Compiles the three parts of the chain for each distinct top, side and bottom art.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let art = Art {
        top: materials[BlockFace::Up as usize],
        side: materials[BlockFace::North as usize],
        bottom: materials[BlockFace::Down as usize],
    };
    let key = [art.top, art.side, art.bottom];
    let base = if let Some(&base) = templates.get(&key) {
        base
    } else {
        let [foot, poles, crown] = parts(art);
        let base = push_model_template(
            foot,
            MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | MODEL_TEMPLATE_FLAG_SCAFFOLDING,
            storage.templates,
            storage.quads,
        )?;
        push_model_template(
            poles,
            MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
            storage.templates,
            storage.quads,
        )?;
        push_model_template(crown, 0, storage.templates, storage.quads)?;
        templates.insert(key, base);
        base
    };
    set_model_visual(&mut visual, materials, base);
    Ok(CompileRuleResult::Compiled(visual))
}

/// The three textures the model reads.
#[derive(Clone, Copy)]
struct Art {
    top: u32,
    side: u32,
    bottom: u32,
}

/// The foot ring, the corner poles and the crown of rails with their plate.
fn parts(art: Art) -> [Vec<ModelQuad>; 3] {
    let foot = [
        (Flip::R0, [INSET, 0, 0]),
        (Flip::R1, [FAR, 0, INSET]),
        (Flip::R3, [INSET, 0, FAR]),
        (Flip::R2, [0, 0, INSET]),
    ]
    .into_iter()
    .flat_map(|(flip, origin)| rail(art, flip, origin, true))
    .collect();
    let poles = [
        (Flip::R3, [0, 0]),
        (Flip::R1, [FAR, FAR]),
        (Flip::R2, [FAR, 0]),
        (Flip::R0, [0, FAR]),
    ]
    .into_iter()
    .flat_map(|(flip, corner)| pole(art, flip, corner))
    .collect();
    let mut crown: Vec<_> = [
        (Flip::R3, [INSET, FAR, FAR]),
        (Flip::R0, [INSET, FAR, 0]),
        (Flip::R2, [0, FAR, INSET]),
        (Flip::R1, [FAR, FAR, INSET]),
    ]
    .into_iter()
    .flat_map(|(flip, origin)| rail(art, flip, origin, false))
    .collect();
    crown.extend(plate(art));
    [foot, poles, crown]
}

/// One corner pole, whose side faces show the pole columns at the edge of the side art.
fn pole(art: Art, flip: Flip, [x, z]: [i16; 2]) -> Vec<ModelQuad> {
    let [low, high] = [[x, 0, z], [x + INSET, FULL, z + INSET]];
    // Each pole shows the left column on two faces and the right column on the other two.
    let left_faces = match flip {
        Flip::R0 => [BlockFace::East, BlockFace::South],
        Flip::R1 => [BlockFace::North, BlockFace::East],
        Flip::R2 => [BlockFace::North, BlockFace::West],
        Flip::R3 => [BlockFace::West, BlockFace::South],
    };
    BlockFace::ALL
        .into_iter()
        .map(|face| match face {
            BlockFace::Up => textured(art.top, face, low, high, positional(low, high), 0),
            BlockFace::Down => textured(art.bottom, face, low, high, positional(low, high), 0),
            _ => {
                let u = if left_faces.contains(&face) {
                    [0, BAND]
                } else {
                    [BAND_END, WHOLE]
                };
                textured(art.side, face, low, high, [u[0], u[1], 0, WHOLE], 0)
            }
        })
        .map(culled_at_block_edge)
        .collect()
}

/// One rail of a ring, without end faces. The foot ring reads the lower band of the side art.
fn rail(art: Art, flip: Flip, origin: [i16; 3], foot: bool) -> Vec<ModelQuad> {
    let long = FAR - INSET;
    let (size_x, size_z, front, back) = match flip {
        Flip::R0 => (long, INSET, BlockFace::South, BlockFace::North),
        Flip::R1 => (INSET, long, BlockFace::West, BlockFace::East),
        Flip::R2 => (INSET, long, BlockFace::East, BlockFace::West),
        Flip::R3 => (long, INSET, BlockFace::North, BlockFace::South),
    };
    let rotation = flip as u8;
    let low = origin;
    let high = [origin[0] + size_x, origin[1] + INSET, origin[2] + size_z];
    let band = if foot { [BAND_END, WHOLE] } else { [0, BAND] };
    let mirrored = |mirror: bool| u8::from(!foot && mirror) * MIRRORED;
    let side = [BAND, BAND_END, band[0], band[1]];
    let up = if foot {
        // The upward face of a foot rail reads the matching end of the bottom art.
        let rows = if matches!(flip, Flip::R1 | Flip::R2) {
            [BAND_END, WHOLE]
        } else {
            [0, BAND]
        };
        textured(
            art.bottom,
            BlockFace::Up,
            low,
            high,
            [BAND, BAND_END, rows[0], rows[1]],
            rotation,
        )
    } else {
        textured(art.top, BlockFace::Up, low, high, positional(low, high), 0)
    };
    let down = textured(
        art.bottom,
        BlockFace::Down,
        low,
        high,
        [BAND, BAND_END, BAND_END, WHOLE],
        rotation,
    );
    [
        up,
        down,
        textured(
            art.side,
            front,
            low,
            high,
            side,
            mirrored(matches!(flip, Flip::R1 | Flip::R3)),
        ),
        textured(
            art.side,
            back,
            low,
            high,
            side,
            mirrored(matches!(flip, Flip::R1 | Flip::R2)),
        ),
    ]
    .into_iter()
    .map(culled_at_block_edge)
    .collect()
}

/// The thin plate under the crown, reading the middle of the top art on both faces.
fn plate(art: Art) -> Vec<ModelQuad> {
    let [low, high] = [[INSET, PLATE_LOW, INSET], [FAR, FULL, FAR]];
    let inset = [BAND, BAND_END, BAND, BAND_END];
    BlockFace::ALL
        .into_iter()
        .map(|face| match face {
            BlockFace::Up => textured(art.top, face, low, high, inset, 0),
            BlockFace::Down => textured(art.top, face, low, high, inset, 1),
            _ => textured(art.side, face, low, high, [BAND, BAND_END, 0, 16], 0),
        })
        .map(culled_at_block_edge)
        .collect()
}

/// The rect that gives an Up or Down face its position in the block.
fn positional(low: [i16; 3], high: [i16; 3]) -> Rect {
    [uv(low[0]), uv(high[0]), uv(low[2]), uv(high[2])]
}

/// Converts a model position to a texture coordinate.
fn uv(position: i16) -> u16 {
    position as u16 * 16
}

/// Native flip value that mirrors a side face.
const MIRRORED: u8 = 4;

/// Builds one face of a cuboid, assigning `rect` by the native corner order for its flip.
fn textured(
    material: u32,
    face: BlockFace,
    low: [i16; 3],
    high: [i16; 3],
    rect: Rect,
    flip: u8,
) -> ModelQuad {
    let mut quad = cuboid_quads([material; 6], low, high)[face as usize];
    let [u0, u1, v0, v1] = rect;
    let (u0, u1) = if flip == MIRRORED { (u1, u0) } else { (u0, u1) };
    for (position, uv) in quad.positions.iter().zip(&mut quad.uvs) {
        let far_x = position[0] == high[0];
        let far_y = position[1] == high[1];
        let far_z = position[2] == high[2];
        *uv = match face {
            BlockFace::North => [if far_x { u0 } else { u1 }, if far_y { v0 } else { v1 }],
            BlockFace::South => [if far_x { u1 } else { u0 }, if far_y { v0 } else { v1 }],
            BlockFace::West => [if far_z { u1 } else { u0 }, if far_y { v0 } else { v1 }],
            BlockFace::East => [if far_z { u0 } else { u1 }, if far_y { v0 } else { v1 }],
            BlockFace::Up => match (flip, far_x, far_z) {
                (1, false, false) => [u0, v1],
                (1, true, false) => [u0, v0],
                (1, true, true) => [u1, v0],
                (1, false, true) => [u1, v1],
                (2, false, false) => [u1, v0],
                (2, true, false) => [u1, v1],
                (2, true, true) => [u0, v1],
                (2, false, true) => [u0, v0],
                (3, false, false) => [u1, v1],
                (3, true, false) => [u0, v1],
                (3, true, true) => [u0, v0],
                (3, false, true) => [u1, v0],
                (_, x, z) => [if x { u1 } else { u0 }, if z { v1 } else { v0 }],
            },
            BlockFace::Down => match (flip, far_x, far_z) {
                (1, false, false) => [u1, v1],
                (1, true, false) => [u1, v0],
                (1, true, true) => [u0, v0],
                (1, false, true) => [u0, v1],
                (2, false, false) => [u0, v0],
                (2, true, false) => [u0, v1],
                (2, true, true) => [u1, v1],
                (2, false, true) => [u1, v0],
                (3, false, false) => [u1, v0],
                (3, true, false) => [u0, v0],
                (3, true, true) => [u0, v1],
                (3, false, true) => [u1, v1],
                (_, x, z) => [if x { u1 } else { u0 }, if z { v0 } else { v1 }],
            },
        };
    }
    quad
}

/// Lets a neighbouring full face hide a quad that lies on the block's boundary.
fn culled_at_block_edge(mut quad: ModelQuad) -> ModelQuad {
    let on_edge = (0..3).any(|axis| {
        let first = quad.positions[0][axis];
        (first == 0 || first == FULL) && quad.positions.iter().all(|p| p[axis] == first)
    });
    if on_edge {
        quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
    }
    quad
}

#[cfg(test)]
#[path = "scaffolding_tests.rs"]
mod tests;
