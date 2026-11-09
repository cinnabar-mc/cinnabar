//! Spore blossoms hang a flat base under the ceiling and droop four petals past their cell.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

const FLAP_TOP: i16 = 248;
const FLAP_TIP: i16 = 120;
const BASE: i16 = 254;
const NEAR: i16 = 128;
const REACH: i16 = 384;
const BACK: i16 = -128;
const FULL_UV: u16 = 4096;

/// A petal's corners and the art corner each one samples.
type Petal = ([[i16; 3]; 4], [[u16; 2]; 4]);

/// The four petals; the art's top row is the tip.
fn petals() -> [Petal; 4] {
    [
        (
            [
                [0, FLAP_TIP, REACH],
                [256, FLAP_TIP, REACH],
                [256, FLAP_TOP, NEAR],
                [0, FLAP_TOP, NEAR],
            ],
            [[0, 0], [FULL_UV, 0], [FULL_UV, FULL_UV], [0, FULL_UV]],
        ),
        (
            [
                [0, FLAP_TIP, BACK],
                [256, FLAP_TIP, BACK],
                [256, FLAP_TOP, NEAR],
                [0, FLAP_TOP, NEAR],
            ],
            [[FULL_UV, 0], [0, 0], [0, FULL_UV], [FULL_UV, FULL_UV]],
        ),
        (
            [
                [NEAR, FLAP_TOP, 0],
                [BACK, FLAP_TIP, 0],
                [BACK, FLAP_TIP, 256],
                [NEAR, FLAP_TOP, 256],
            ],
            [[0, FULL_UV], [0, 0], [FULL_UV, 0], [FULL_UV, FULL_UV]],
        ),
        (
            [
                [REACH, FLAP_TIP, 0],
                [NEAR, FLAP_TOP, 0],
                [NEAR, FLAP_TOP, 256],
                [REACH, FLAP_TIP, 256],
            ],
            [[0, 0], [0, FULL_UV], [FULL_UV, FULL_UV], [FULL_UV, 0]],
        ),
    ]
}

/// Four petals on the `Down` art and the flat base on the `Up` art, all two-sided.
fn spore_blossom_quads(petal: u32, base: u32) -> Vec<ModelQuad> {
    let mut quads: Vec<_> = petals()
        .into_iter()
        .map(|(positions, uvs)| ModelQuad {
            positions,
            uvs,
            material: petal,
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        })
        .collect();
    quads.push(ModelQuad {
        positions: [
            [0, BASE, 256],
            [256, BASE, 256],
            [256, BASE, 0],
            [0, BASE, 0],
        ],
        uvs: [[0, 0], [FULL_UV, 0], [FULL_UV, FULL_UV], [0, FULL_UV]],
        material: base,
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    });
    quads
}

/// Whether the record is the spore blossom, whose exact rule replaces its provisional envelope.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::SporeBlossom.identifier()
}

/// Compiles the blossom from its petal and base art; packs without that art get the diagnostic
/// visual.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 2], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::NoMatch);
    };
    let key = [BlockFace::Down, BlockFace::Up].map(|face| materials[face as usize]);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            spore_blossom_quads(key[0], key[1]),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    let mut visual = diagnostic_visual(record);
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}
