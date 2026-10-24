use super::*;
use crate::chunk::transparent::mixed::plan::{MixedFace, MixedStream, sort_faces};
use meshing::{Face, PackedBiomeRecord, PackedModelDrawRef, PackedModelRef, PackedQuad};
use render_scene::ChunkUploadPriority;
use std::sync::Arc;

const TAG: u8 = 2;

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

/// A section at the world origin with `cubes` and translucent model draws of one block's two
/// template quads, the draws tagged with `tag`.
fn section(cubes: Vec<PackedQuad>, tag: u8) -> ChunkRenderInstance {
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
        liquid_quads: Arc::from([]),
        liquid_lighting: Arc::from([]),
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: ChunkBiomeTintIdentity::default(),
        generation: 1,
        priority: ChunkUploadPriority::new(0.0),
        token: None,
        publication_permit: None,
        origin: [0; 3],
    }
}

#[test]
fn tagged_model_draws_keep_their_ordering_centroid_and_tag() {
    let (templates, quads) = templates();
    let instance = section(Vec::new(), 0);
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
fn bound_faces_draw_back_to_front_like_ordinary_translucent_faces() {
    let (templates, quads) = templates();
    // Each list runs front to back for a camera north of the section.
    let cubes = vec![
        PackedQuad::new([4, 0, 1], Face::NegativeZ, 1, 1, 0).with_world_material(TAG),
        PackedQuad::new([4, 0, 3], Face::NegativeZ, 1, 1, 0),
        PackedQuad::new([4, 0, 6], Face::NegativeZ, 1, 1, 0).with_world_material(TAG),
        PackedQuad::new([4, 0, 9], Face::NegativeZ, 1, 1, 0).with_world_material(TAG + 1),
    ];
    let instance = section(cubes, TAG);
    let camera = Vec3::new(4.5, 0.5, -3.0);
    let faces = tagged_faces(&instance, &templates, &quads, TAG);
    assert_eq!(
        faces.len(),
        4,
        "only this material's faces, models included"
    );
    let segments = order(instance.key, camera, faces.clone());
    assert_eq!(
        segments,
        [
            (WorldStream::Cube, 2..3),
            (WorldStream::TransparentModel, 1..2),
            (WorldStream::TransparentModel, 0..1),
            (WorldStream::Cube, 0..1),
        ]
        .map(|(stream, range)| WorldSegment { stream, range }),
        "farthest first, never list order"
    );
    // The model faces keep the order ordinary translucent terrain gives them.
    let mut ordinary = faces
        .iter()
        .filter(|face| face.stream == WorldStream::TransparentModel)
        .map(|face| MixedFace {
            stream: MixedStream::Model,
            index: face.index,
            centroid: face.centroid,
            stable: [0, face.index],
        })
        .collect::<Vec<_>>();
    sort_faces(instance.key, camera, &mut ordinary);
    let bound = segments
        .iter()
        .filter(|segment| segment.stream == WorldStream::TransparentModel)
        .flat_map(|segment| segment.range.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        bound,
        ordinary.iter().map(|face| face.index).collect::<Vec<_>>()
    );
}

#[test]
fn consecutive_faces_of_one_list_merge_into_one_draw() {
    let cubes = (0..3)
        .map(|z| PackedQuad::new([0, 0, 10 - z], Face::NegativeZ, 1, 1, 0).with_world_material(TAG))
        .collect();
    let instance = section(cubes, 0);
    let faces = tagged_faces(&instance, &[], &[], TAG);
    assert_eq!(
        order(instance.key, Vec3::new(0.5, 0.5, -3.0), faces),
        [WorldSegment {
            stream: WorldStream::Cube,
            range: 0..3,
        }]
    );
}

#[test]
fn cube_centroids_sit_at_the_middle_of_the_emitted_face() {
    let quad = PackedQuad::new([1, 2, 3], Face::PositiveY, 4, 2, 0);
    assert_eq!(
        cube_quad_centroid([16, 0, -16], quad),
        Vec3::new(16.0 + 1.0 + 2.0, 2.0 + 1.0, -16.0 + 3.0 + 1.0)
    );
    let quad = PackedQuad::new([1, 2, 3], Face::NegativeX, 4, 2, 0);
    assert_eq!(cube_quad_centroid([0; 3], quad), Vec3::new(1.0, 3.0, 5.0));
}
