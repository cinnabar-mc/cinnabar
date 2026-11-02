//! Placed candles: one to four native candle cuboids, each with its crossed wick.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_grouped_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockProperty, BlockRenderKind as R, block_render_fact};

/// Identifies the undyed candle and the sixteen dyed candles.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Candle
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// The zero-based candle count and lit flag, admitting only those two typed selectors.
fn selectors(record: &RegistryRecord) -> Option<(u8, bool)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let count = super::state::exact_tagged_int(state.get(BlockProperty::Candles.name())?, 3)?;
    let lit = super::state::exact_tagged_byte(state.get(BlockProperty::Lit.name())?, 1)?;
    Some((count as u8, lit != 0))
}

/// The candle sprite of `record`, picking the lit terrain variant when `lit`, as native
/// tessellation selects it from the lit state.
pub(in crate::compiler) fn descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    lit: bool,
) -> Option<(Descriptor, Box<str>)> {
    let TextureKey { key, rotate_uv } =
        resolve_texture_key(&pack.blocks, record, assets::BlockFace::Down);
    let key = key?;
    let (path, state_variant) = pack
        .terrain
        .get_clamped_untinted_variant(&key, usize::from(lit))?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT
                | if rotate_uv {
                    MATERIAL_FLAG_ROTATE_UV
                } else {
                    0
                },
        },
        key,
    ))
}

/// Resolves the placed candle's sprite for the material pass.
pub(in crate::compiler) fn descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<Vec<(Descriptor, Box<str>)>> {
    let (_, lit) = selectors(record)?;
    Some(vec![descriptor(pack, record, lit)?])
}

/// Compiles every candle count and lit state, sharing templates between equal sprites.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<(u32, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((count, lit)) = selectors(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(material) = descriptor(inputs.pack, record, lit)
        .and_then(|(descriptor, _)| inputs.material_by_descriptor.get(&descriptor).copied())
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (material, count);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        // Each candle stays in one part: its wick takes its light from the candle cuboid.
        let template = push_grouped_model_template(
            assets::facts::candle::placed_candles(count)
                .iter()
                .map(|&(height, [x, z])| {
                    candle_quads(material, [position(x), position(z)], 0, height)
                }),
            storage,
        )?
        .expect("every candle count places a candle");
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, [material; 6], template);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Block coordinates in the carrier's 1/256 block units; candle centres are exact there.
fn position(value: f32) -> i16 {
    (value * f32::from(assets::MODEL_POSITION_SCALE)) as i16
}

/// One native candle: a two-pixel-wide cuboid `height` pixels tall on `base`, centred on
/// `center`, followed by both sides of its two diagonal wick planes. The sides sample sprite
/// rows from 8 down by the candle's height, the top rows 6-8, the bottom rows 14-16 and the
/// wick row 5. The wick follows the cuboid without its own shade, as native tessellation draws it.
pub(in crate::compiler) fn candle_quads(
    material: u32,
    [x, z]: [i16; 2],
    base: i16,
    height: u16,
) -> Vec<ModelQuad> {
    const PIXEL: i16 = assets::MODEL_POSITION_SCALE / 16;
    let top = base + height as i16 * PIXEL;
    let mut quads = Vec::with_capacity(10);
    for (face, mut quad) in
        assets::BlockFace::ALL
            .into_iter()
            .zip(super::geometry::vanilla_cuboid_quads(
                [material; 6],
                [x - PIXEL, base, z - PIXEL],
                [x + PIXEL, top, z + PIXEL],
            ))
    {
        let region = match face {
            assets::BlockFace::Down => [0, 14, 2, 16],
            assets::BlockFace::Up => [0, 6, 2, 8],
            _ => [0, 8, 2, 8 + height],
        };
        for axis in 0..2 {
            let min = quad.uvs.iter().map(|uv| uv[axis]).min().unwrap();
            let max = quad.uvs.iter().map(|uv| uv[axis]).max().unwrap();
            for uv in &mut quad.uvs {
                uv[axis] = region[axis] * 256
                    + (u32::from(uv[axis] - min)
                        * u32::from((region[axis + 2] - region[axis]) * 256)
                        / u32::from(max - min)) as u16;
            }
        }
        quads.push(quad);
    }
    let half = PIXEL / 2;
    for positions in [
        [
            [x - half, top + PIXEL, z - half],
            [x - half, top, z - half],
            [x + half, top, z + half],
            [x + half, top + PIXEL, z + half],
        ],
        [
            [x - half, top + PIXEL, z + half],
            [x - half, top, z + half],
            [x + half, top, z - half],
            [x + half, top + PIXEL, z - half],
        ],
    ] {
        let quad = ModelQuad {
            positions,
            uvs: [[0, 1280], [0, 1536], [256, 1536], [256, 1280]],
            material,
            flags: assets::MODEL_QUAD_FLAG_INHERITED_SHADE,
        };
        quads.push(quad);
        let mut back = quad;
        back.positions.reverse();
        back.uvs.reverse();
        quads.push(back);
    }
    quads
}
