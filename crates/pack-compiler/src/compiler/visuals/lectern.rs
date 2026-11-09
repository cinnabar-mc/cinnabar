//! The lectern stand. Terrain owns the stand's quads, reading the block's own face materials,
//! and the block-entity renderer draws only the book on top.
//!
//! Every direction is the shared north-facing stand turned about the block's centre, so the
//! art in `lectern_faces` is the one description of the stand.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::block_entity_geometry::{LECTERN_TEXTURES, lectern_faces};
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// The block face whose material supplies each entry of [`LECTERN_TEXTURES`].
const TEXTURE_FACES: [BlockFace; LECTERN_TEXTURES.len()] = [
    BlockFace::Down,
    BlockFace::East,
    BlockFace::South,
    BlockFace::Up,
    BlockFace::North,
];
/// The quad face ids of a north-facing stand's faces, in `lectern_faces` order.
const FACE_IDS: [u32; 6] = [3, 4, 1, 2, 5, 6];
/// Template units per pixel, and per texel of a 16-texel tile.
const POSITION_UNITS_PER_PIXEL: f32 = 16.0;
const UV_UNITS_PER_TEXEL: f32 = 256.0;

/// Identifies the lectern by its render route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Lectern
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads the quarter turns from north a state's direction asks for.
fn turns(record: &RegistryRecord) -> Option<u8> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    match super::state::exact_tagged_string(
        state.get(assets::facts::BlockProperty::MinecraftCardinalDirection.name())?,
    )? {
        "north" => Some(0),
        "west" => Some(1),
        "south" => Some(2),
        "east" => Some(3),
        _ => None,
    }
}

/// Compiles every direction from the pack's own face materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(turns) = turns(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, turns);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template =
            push_model_template(quads(materials, turns), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Builds the stand's quads, turned counter-clockwise from above by `turns` quarter turns.
fn quads(materials: [u32; 6], turns: u8) -> Vec<ModelQuad> {
    lectern_faces()
        .into_iter()
        .enumerate()
        .map(|(index, (texture, (corners, [u, v, width, height])))| {
            let corner_uvs = [
                [u, v],
                [u + width, v],
                [u + width, v + height],
                [u, v + height],
            ];
            // Terrain winds the other way from the authored faces.
            let mut positions = corners.map(|corner| position(corner, turns));
            let mut uvs = corner_uvs.map(|uv| uv.map(|axis| (axis * UV_UNITS_PER_TEXEL) as u16));
            positions.reverse();
            uvs.reverse();
            ModelQuad {
                positions,
                uvs,
                material: materials[TEXTURE_FACES[texture] as usize],
                flags: turned_face(FACE_IDS[index % 6], turns),
            }
        })
        .collect()
}

/// Moves an authored pixel from around the block's centre into a block's template units.
fn position([x, y, z]: [f32; 3], turns: u8) -> [i16; 3] {
    let (x, z) = (0..turns).fold((x, z), |(x, z), _| (z, -x));
    [x + 8.0, y, z + 8.0].map(|axis| (axis * POSITION_UNITS_PER_PIXEL).round() as i16)
}

/// The quad face id a north-facing side moves to after `turns`; caps stay where they are.
fn turned_face(face: u32, turns: u8) -> u32 {
    const ROUND: [u32; 4] = [5, 3, 6, 4];
    ROUND
        .iter()
        .position(|&side| side == face)
        .map_or(face, |side| ROUND[(side + usize::from(turns)) % 4])
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::MODEL_QUAD_FLAG_FACE_MASK;

    const MATERIALS: [u32; 6] = [10, 11, 12, 13, 14, 15];

    /// Quads of the post, the second part of the stand.
    fn post(turns: u8) -> Vec<ModelQuad> {
        quads(MATERIALS, turns)[6..12].to_vec()
    }

    /// The quad on `face` among `quads`.
    fn on_face(quads: &[ModelQuad], face: u32) -> &ModelQuad {
        quads
            .iter()
            .find(|quad| quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face)
            .unwrap()
    }

    /// The UV at the corner of `quad` nearest a block-unit `corner`.
    fn uv_at(quad: &ModelQuad, corner: [i16; 3]) -> [u16; 2] {
        let index = quad
            .positions
            .iter()
            .position(|position| *position == corner)
            .unwrap();
        quad.uvs[index].map(|axis| axis / 256)
    }

    /// The outward normal of a quad's winding.
    fn normal(quad: &ModelQuad) -> [f32; 3] {
        let [a, b, c, _] = quad.positions.map(|p| p.map(f32::from));
        let (u, v) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        );
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    }

    #[test]
    fn post_art_stands_upright_with_the_ribbon_on_its_front() {
        let quads = post(0);
        let front = on_face(&quads, 5);
        assert_eq!(front.material, MATERIALS[BlockFace::North as usize]);
        // The front runs its 8-wide art across the post, top left at the post's top east corner.
        assert_eq!(uv_at(front, [192, 240, 64]), [0, 0]);
        assert_eq!(uv_at(front, [64, 240, 64]), [8, 0]);
        assert_eq!(uv_at(front, [64, 32, 64]), [8, 13]);
        let back = on_face(&quads, 6);
        assert_eq!(uv_at(back, [64, 240, 192]), [8, 3]);
        assert_eq!(uv_at(back, [192, 32, 192]), [16, 16]);
    }

    #[test]
    fn post_flanks_turn_their_long_grain_up_the_post() {
        let quads = post(0);
        let west = on_face(&quads, 3);
        assert_eq!(west.material, MATERIALS[BlockFace::South as usize]);
        assert_eq!(uv_at(west, [64, 240, 192]), [2, 8]);
        assert_eq!(uv_at(west, [64, 32, 192]), [15, 8]);
        let east = on_face(&quads, 4);
        assert_eq!(uv_at(east, [192, 240, 64]), [2, 16]);
        assert_eq!(uv_at(east, [192, 32, 64]), [15, 16]);
    }

    #[test]
    fn base_and_board_read_their_own_materials() {
        let quads = quads(MATERIALS, 0);
        let [base, _, board] = [&quads[..6], &quads[6..12], &quads[12..18]];
        assert_eq!(
            on_face(base, 2).material,
            MATERIALS[BlockFace::East as usize]
        );
        assert_eq!(
            on_face(base, 1).material,
            MATERIALS[BlockFace::Down as usize]
        );
        assert_eq!(
            on_face(base, 5).material,
            MATERIALS[BlockFace::East as usize]
        );
        assert_eq!(
            on_face(board, 2).material,
            MATERIALS[BlockFace::Up as usize]
        );
        assert_eq!(
            on_face(board, 1).material,
            MATERIALS[BlockFace::Down as usize]
        );
    }

    #[test]
    fn the_stand_turns_rigidly_and_stays_wound_outward() {
        for turns in 0..4 {
            let quads = quads(MATERIALS, turns);
            for (index, quad) in quads[..12].iter().enumerate() {
                let [x, y, z] = normal(quad);
                let expected = match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
                    1 => y < 0.0,
                    2 => y > 0.0,
                    3 => x < 0.0,
                    4 => x > 0.0,
                    5 => z < 0.0,
                    _ => z > 0.0,
                };
                assert!(expected, "quad {index} after {turns} turns");
            }
            let front = on_face(&quads[6..12], [5, 3, 6, 4][usize::from(turns)]);
            assert_eq!(front.material, MATERIALS[BlockFace::North as usize]);
        }
        // Turning west moves the top east corner of the front to the top north corner.
        let west_front = *on_face(&post(1), 3);
        assert_eq!(uv_at(&west_front, [64, 240, 64]), [0, 0]);
    }

    #[test]
    fn the_board_leans_toward_its_reader() {
        let quads = quads(MATERIALS, 0);
        let top = on_face(&quads[12..18], 2);
        let height = |side: bool| {
            top.positions
                .iter()
                .filter(|position| (position[2] < 128) == side)
                .map(|position| position[1])
                .max()
                .unwrap()
        };
        assert!(height(true) < height(false), "the near edge sits lower");
    }
}
