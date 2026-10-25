use super::*;
use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream, sort_faces};
use meshing::{
    Face as Side, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef,
    PackedQuad,
};
use render_scene::ChunkUploadPriority;
use std::sync::Arc;

const TAG: u8 = 2;
/// The translucent slot `TAG` names.
const SLOTS: u64 = 1 << (TAG - 1);

/// Two template quads of one block: index 0 at the block's north face, index 1 at its south.
fn templates() -> ([assets::ModelTemplate; 1], [assets::ModelQuad; 2]) {
    let quad = |positions| assets::ModelQuad {
        positions,
        uvs: [[0; 2]; 4],
        material: 0,
        flags: 0,
    };
    (
        [assets::ModelTemplate {
            quad_start: 0,
            quad_count: 2,
            flags: 0,
        }],
        [
            quad([[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]]),
            quad([[0, 0, 256], [0, 256, 256], [256, 256, 256], [256, 0, 256]]),
        ],
    )
}

/// A section at the world origin with `cubes`, `water` and translucent model draws of one
/// block's two template quads, the draws tagged with `tag`.
fn section(cubes: Vec<PackedQuad>, water: Vec<PackedLiquidQuad>, tag: u8) -> ChunkRenderInstance {
    let draws = [0, 1].map(|quad| PackedModelDrawRef::new(0, quad).with_world_material(tag));
    ChunkRenderInstance {
        key: SubChunkKey::new(0, 0, 0, 0),
        cube_quads: cubes.into(),
        cube_lighting: Arc::from([]),
        cube_layout: Default::default(),
        model_refs: Arc::from([PackedModelRef::new(0, 0, 0, 0b11)]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from(draws),
        has_transparent_liquid: !water.is_empty(),
        liquid_lighting: Arc::from([]),
        liquid_quads: water.into(),
        has_depth_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [0; 3],
        world_materials: false,
    }
}

/// The section's translucent model draws in mesh order, as absolute GPU words.
fn mesh_words(instance: &ChunkRenderInstance) -> Vec<[u32; 2]> {
    instance
        .transparent_model_draw_refs
        .iter()
        .map(|draw| draw.words())
        .collect()
}

/// `instance`'s merged order for `camera`, with the given GPU model order.
fn merged(instance: &ChunkRenderInstance, words: &[[u32; 2]], camera: Vec3) -> Vec<Segment> {
    let (templates, quads) = templates();
    let faces = collect_faces(&SectionFaces {
        instance,
        model_words: words,
        model_base: 0,
        templates: &templates,
        model_quads: &quads,
        slots: SLOTS,
    });
    order(instance.key, camera, faces)
}

/// A south-facing water face at local `origin`.
fn water(origin: [u8; 3]) -> PackedLiquidQuad {
    PackedLiquidQuad::try_pack(origin, Side::NegativeZ, [255; 4], 0, 0, [0; 2], false).unwrap()
}

/// `segments` as plain (stream, range) pairs.
fn runs(segments: &[Segment]) -> Vec<(Stream, Range<u32>)> {
    segments
        .iter()
        .map(|segment| (segment.stream, segment.range.clone()))
        .collect()
}

const fn world(list: WorldList) -> Stream {
    Stream::World {
        slot: (TAG - 1) as u16,
        list,
    }
}

#[test]
fn tagged_model_draws_keep_their_ordering_centroid_and_tag() {
    let (templates, quads) = templates();
    let instance = section(Vec::new(), Vec::new(), 0);
    let candidate = |draw: PackedModelDrawRef| {
        crate::chunk::transparent::model::transparent_model_draw_candidate(
            instance.key,
            &instance.model_refs,
            &instance.model_lighting,
            draw,
            &templates,
            &quads,
            0,
        )
    };
    let plain = PackedModelDrawRef::new(0, 1);
    let tagged = plain.with_world_material(TAG);
    let (plain_centroid, _) = candidate(plain).expect("an untagged draw sorts");
    let (centroid, words) = candidate(tagged).expect("a tagged draw sorts like Bedrock");
    assert_eq!(centroid, plain_centroid);
    assert_eq!(words, tagged.words(), "the sorted draw ref keeps its tag");
}

#[test]
fn water_and_a_bound_translucent_block_in_one_section_blend_in_one_order() {
    // From north to south: water at z 1, a bound cube face at z 4, water at z 7, and an
    // ordinary glass-like block at the section corner. The camera looks from the north, so
    // the far water draws first, then the bound face, the glass, and the near water last;
    // never all of one kind before another.
    let cube = PackedQuad::new([4, 0, 4], Side::NegativeZ, 1, 1, 0).with_world_material(TAG);
    let instance = section(vec![cube], vec![water([4, 0, 1]), water([4, 0, 7])], 0);
    let camera = Vec3::new(4.5, 0.5, -3.0);
    assert_eq!(
        runs(&merged(&instance, &mesh_words(&instance), camera)),
        [
            (Stream::Water, 1..2),
            (world(WorldList::Cube), 0..1),
            (Stream::Model, 1..2),
            (Stream::Model, 0..1),
            (Stream::Water, 0..1),
        ]
    );
}

#[test]
fn bound_faces_sort_like_ordinary_translucent_faces() {
    // Each list runs front to back for a camera north of the section.
    let cubes = vec![
        PackedQuad::new([4, 0, 1], Side::NegativeZ, 1, 1, 0).with_world_material(TAG),
        PackedQuad::new([4, 0, 3], Side::NegativeZ, 1, 1, 0),
        PackedQuad::new([4, 0, 6], Side::NegativeZ, 1, 1, 0).with_world_material(TAG),
        PackedQuad::new([4, 0, 9], Side::NegativeZ, 1, 1, 0).with_world_material(TAG + 1),
    ];
    let instance = section(cubes, Vec::new(), TAG);
    let camera = Vec3::new(4.5, 0.5, -3.0);
    let segments = merged(&instance, &mesh_words(&instance), camera);
    assert_eq!(
        runs(&segments),
        [
            (world(WorldList::Cube), 2..3),
            (world(WorldList::TransparentModel), 1..2),
            (world(WorldList::TransparentModel), 0..1),
            (world(WorldList::Cube), 0..1),
        ],
        "farthest first; untagged and other-slot cubes stay out"
    );
    // The model faces keep the order ordinary translucent terrain gives them.
    let (templates, quads) = templates();
    let mut ordinary = (0..2)
        .map(|index| MixedFace {
            stream: MixedStream::Model,
            index,
            centroid: crate::chunk::transparent::model::transparent_model_draw_candidate(
                instance.key,
                &instance.model_refs,
                &instance.model_lighting,
                instance.transparent_model_draw_refs[index as usize],
                &templates,
                &quads,
                0,
            )
            .unwrap()
            .0,
            stable: [0, index],
        })
        .collect::<Vec<_>>();
    sort_faces(instance.key, camera, &mut ordinary);
    let bound = segments
        .iter()
        .filter(|segment| segment.stream == world(WorldList::TransparentModel))
        .flat_map(|segment| segment.range.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        bound,
        ordinary.iter().map(|face| face.index).collect::<Vec<_>>()
    );
}

#[test]
fn bound_model_draws_are_named_by_their_position_in_the_uploaded_order() {
    // Ordinary sorting rewrites the section's translucent draw list in place, so a bound draw
    // is drawn at the index the uploaded order holds it, not its mesh index.
    let instance = section(Vec::new(), Vec::new(), TAG);
    let mut uploaded = mesh_words(&instance);
    uploaded.reverse();
    let segments = merged(&instance, &uploaded, Vec3::new(4.5, 0.5, -3.0));
    // Mesh index 1 (the south quad, farther) now sits at uploaded index 0.
    assert_eq!(
        runs(&segments),
        [(world(WorldList::TransparentModel), 0..2)],
        "the far face first, at its uploaded position"
    );
}

#[test]
fn consecutive_faces_of_one_list_merge_into_one_draw() {
    let cubes = (0..3)
        .map(|z| PackedQuad::new([0, 0, 10 - z], Side::NegativeZ, 1, 1, 0).with_world_material(TAG))
        .collect();
    let instance = section(cubes, Vec::new(), 0);
    assert_eq!(
        runs(&merged(&instance, &[], Vec3::new(0.5, 0.5, -3.0))),
        [(world(WorldList::Cube), 0..3)]
    );
}

#[test]
fn cube_centroids_sit_at_the_middle_of_the_emitted_face() {
    let quad = PackedQuad::new([1, 2, 3], Side::PositiveY, 4, 2, 0);
    assert_eq!(
        cube_quad_centroid([16, 0, -16], quad),
        Vec3::new(16.0 + 1.0 + 2.0, 2.0 + 1.0, -16.0 + 3.0 + 1.0)
    );
    let quad = PackedQuad::new([1, 2, 3], Side::NegativeX, 4, 2, 0);
    assert_eq!(cube_quad_centroid([0; 3], quad), Vec3::new(1.0, 3.0, 5.0));
}
