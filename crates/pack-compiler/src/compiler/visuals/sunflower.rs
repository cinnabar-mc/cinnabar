//! A sunflower's upper half adds a tilted head to its crossed stalk.

use super::cross::crossed_quads;
use assets::facts::pack::PackBlock as B;
use {super::super::*, assets::BlockFace};

/// Head plane in 1/256 block: its top leans west of its foot and it rises past the block's top.
const HEAD_TOP_X: i16 = 115;
const HEAD_FOOT_X: i16 = 192;
const HEAD_FOOT_Y: i16 = 26;
const HEAD_TOP_Y: i16 = 282;

/// Whether the record is either half of a sunflower.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == B::Sunflower.identifier()
}

/// The face carrying the head's sunny side.
pub(in crate::compiler) const FRONT_FACE: BlockFace = BlockFace::North;
/// The face carrying the head's back.
pub(in crate::compiler) const BACK_FACE: BlockFace = BlockFace::South;

/// Whether this record's face resolves to one side of the head rather than its stalk.
pub(in crate::compiler) fn is_head_face(record: &RegistryRecord, face: BlockFace) -> bool {
    is_record(record) && matches!(face, FRONT_FACE | BACK_FACE)
}

/// Resolves the head's two array entries from the side texture.
pub(in crate::compiler) fn head_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let key = resolve_texture_key(&pack.blocks, record, face).key?;
    let (path, state_variant) = pack
        .terrain
        .get_clamped_untinted_variant(&key, usize::from(face == BACK_FACE))?;
    Some((
        Descriptor {
            path: path.into(),
            texture_key: key.clone(),
            state_variant,
            flipbook_selector: None,
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

/// Whether a record is the upper half that draws the head.
pub(in crate::compiler) fn draws_head(record: &RegistryRecord) -> bool {
    is_record(record)
        && canonical_state_u32(
            &record.canonical_state,
            assets::facts::BlockProperty::UpperBlockBit.name(),
        ) == Some(1)
}

/// The stalk's cross plus two one-sided planes sharing the head's position: the front faces east
/// and up, the back faces west and down, each reading its art left to right from its own side.
pub(in crate::compiler) fn quads([stalk, front, back]: [u32; 3]) -> Vec<ModelQuad> {
    let (foot_v, top_v) = (4096, 0);
    let mut quads = crossed_quads([stalk; 2]).to_vec();
    quads.extend([
        ModelQuad {
            positions: [
                [HEAD_FOOT_X, HEAD_FOOT_Y, 256],
                [HEAD_FOOT_X, HEAD_FOOT_Y, 0],
                [HEAD_TOP_X, HEAD_TOP_Y, 0],
                [HEAD_TOP_X, HEAD_TOP_Y, 256],
            ],
            uvs: [[0, foot_v], [4096, foot_v], [4096, top_v], [0, top_v]],
            material: front,
            flags: 0,
        },
        ModelQuad {
            positions: [
                [HEAD_FOOT_X, HEAD_FOOT_Y, 0],
                [HEAD_FOOT_X, HEAD_FOOT_Y, 256],
                [HEAD_TOP_X, HEAD_TOP_Y, 256],
                [HEAD_TOP_X, HEAD_TOP_Y, 0],
            ],
            uvs: [[0, foot_v], [4096, foot_v], [4096, top_v], [0, top_v]],
            material: back,
            flags: 0,
        },
    ]);
    quads
}
