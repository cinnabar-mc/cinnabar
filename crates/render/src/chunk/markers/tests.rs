use super::*;
use assets::{TerrainMarker, TextureArray, TextureMip, TextureRef};
use meshing::MeshLightSample;

/// The block whose art the test resolver knows, and that art's index.
const ART_BLOCK: u32 = 7;
const ART: u32 = 3;

fn key(x: i32, y: i32, z: i32) -> SubChunkKey {
    SubChunkKey {
        dimension: 0,
        x,
        y,
        z,
    }
}

fn marker(local: [u8; 3], class: TerrainMarker, block: u32) -> PackedTerrainMarker {
    PackedTerrainMarker::new(
        local,
        class,
        block,
        MeshLightSample::try_new(13, 4).unwrap(),
    )
}

fn instances(
    sections: &TerrainMarkerSections,
    layers: u8,
) -> (Vec<MarkerInstance>, Vec<MarkerInstance>) {
    let (mut sprites, mut boxes) = (Vec::new(), Vec::new());
    marker_instances(
        sections,
        layers,
        |block, _| (block == ART_BLOCK).then_some([ART; 6]),
        &mut sprites,
        &mut boxes,
    );
    (sprites, boxes)
}

#[test]
fn only_shown_layers_with_art_become_instances_at_their_world_cells() {
    let mut sections = TerrainMarkerSections::default();
    sections.set(
        key(1, -1, 2),
        &Arc::from([
            marker([3, 4, 5], TerrainMarker::Light, ART_BLOCK),
            marker([0, 0, 0], TerrainMarker::Barrier, ART_BLOCK),
            marker([15, 15, 15], TerrainMarker::StructureVoid, ART_BLOCK),
            marker([1, 1, 1], TerrainMarker::Light, ART_BLOCK + 1),
        ]),
    );

    assert_eq!(instances(&sections, 0), (Vec::new(), Vec::new()));

    let (sprites, boxes) = instances(&sections, TerrainMarker::Light.layer());
    assert!(boxes.is_empty());
    assert_eq!(
        sprites.len(),
        1,
        "a block the pack gives no art draws nothing"
    );
    assert_eq!(sprites[0].block, [19, -12, 37]);
    assert_eq!(sprites[0].art[0], ART);
    assert_eq!((sprites[0].packed >> LIGHT_SHIFT) & 255, 13 | (4 << 4));
    assert_eq!(sprites[0].packed & BOX_FLAG, 0);

    let (sprites, boxes) = instances(&sections, TerrainMarker::StructureVoid.layer());
    assert!(sprites.is_empty());
    assert_eq!(boxes.len(), 1);
    assert_eq!(boxes[0].block, [31, -1, 47]);
    assert_eq!(boxes[0].packed & BOX_FLAG, BOX_FLAG);
}

#[test]
fn section_changes_advance_the_revision_only_when_cells_change() {
    let mut sections = TerrainMarkerSections::default();
    let cells: Arc<[PackedTerrainMarker]> =
        Arc::from([marker([1, 2, 3], TerrainMarker::Barrier, ART_BLOCK)]);
    sections.set(key(0, 0, 0), &cells);
    let revision = sections.revision;
    sections.set(key(0, 0, 0), &Arc::from(cells.to_vec()));
    assert_eq!(sections.revision, revision);

    sections.set(key(0, 0, 0), &Arc::from([]));
    assert!(sections.revision > revision);
    assert!(instances(&sections, u8::MAX).0.is_empty());

    sections.set(key(0, 0, 0), &cells);
    let revision = sections.revision;
    sections.clear();
    assert!(sections.revision > revision);
    assert!(instances(&sections, u8::MAX).0.is_empty());
}

/// Every vertex buffer holds at most one chunk of instances, whatever the batch size, so a
/// world full of markers never needs a binding larger than the device allows.
#[test]
fn draws_split_sprites_and_boxes_across_fixed_size_buffers() {
    let total = 2 * CHUNK_INSTANCES + 5;
    let sprites = CHUNK_INSTANCES + 3;
    let chunk = CHUNK_INSTANCES as u32;
    let draws = chunk_draws(total, sprites).collect::<Vec<_>>();
    assert_eq!(
        draws,
        [(0..chunk, chunk..chunk), (0..3, 3..chunk), (0..0, 0..5),]
    );
    let drawn = draws
        .iter()
        .map(|(sprites, boxes)| sprites.len() + boxes.len())
        .sum::<usize>();
    assert_eq!(drawn, total);
    assert!(chunk_draws(0, 0).next().is_none());
}

/// Flipbook art keeps every frame and its timing, so the GPU steps it with the terrain clock;
/// missing art keeps no frames.
#[test]
fn flipbook_art_keeps_its_frames_and_timing() {
    let texture = |layer| TextureRef::new(1, layer).unwrap();
    let encode = |texture: TextureRef| MarkerTexture {
        page: texture.page(),
        layer: texture.layer(),
        uv_scale: 1.0,
        reserved: 0,
        gradient_scale: [0.25; 2],
    };
    let animations = [Animation {
        frame_start: 1,
        frame_count: 2,
        ticks_per_frame: 3,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags: ANIMATION_FLAG_BLEND,
    }];
    let frames = [texture(9), texture(10), texture(11)];
    let mut tables = MarkerTables::default();

    let still = Material {
        texture: texture(4),
        ..Material::unvaried()
    };
    let animated = Material {
        texture: texture(10),
        animation: 0,
        ..Material::unvaried()
    };
    assert_eq!(tables.push(Some(&still), &animations, &frames, encode), 0);
    assert_eq!(
        tables.push(Some(&animated), &animations, &frames, encode),
        1
    );
    assert_eq!(tables.push(None, &animations, &frames, encode), 2);

    assert_eq!(
        tables.animations,
        [
            MarkerAnimation {
                frame_start: 0,
                frame_count: 1,
                ticks_per_frame: 1,
                flags: 0,
            },
            MarkerAnimation {
                frame_start: 1,
                frame_count: 2,
                ticks_per_frame: 3,
                flags: ANIMATION_FLAG_BLEND,
            },
            // A face without art has no frames, so it draws nothing.
            MarkerAnimation {
                frame_start: 3,
                frame_count: 0,
                ticks_per_frame: 1,
                flags: 0,
            },
        ]
    );
    assert_eq!(
        tables
            .textures
            .iter()
            .map(|texture| texture.layer)
            .collect::<Vec<_>>(),
        [4, 10, 11]
    );
}

/// Marker art with weighted alternatives picks the one terrain would draw at each world cell,
/// face by face, while faces without art stay empty.
#[test]
fn weighted_marker_art_picks_each_cells_alternative() {
    let leaf = |weight: f32| Material {
        variation_weight: weight.to_bits(),
        ..Material::unvaried()
    };
    let materials = [
        Material::unvaried(),
        Material {
            variation_start: 2,
            variation_count: 2,
            ..Material::unvaried()
        },
        leaf(0.25),
        leaf(0.75),
    ];
    let mut faces = [DIAGNOSTIC_MATERIAL; 6];
    faces[3] = 1;
    let faces = MarkerFaces {
        materials: faces,
        count: 6,
    };
    let at = |position| faces.at(&materials, position).materials;
    let expected = |pick| {
        let mut faces = [DIAGNOSTIC_MATERIAL; 6];
        faces[3] = pick;
        faces
    };
    assert_eq!(at([0, 0, 0]), expected(2));
    assert_eq!(at([-1, 0, -1]), expected(3));
}

/// Gridded pack art samples its exposed cell and picks mips by that cell's source pixels, the
/// same per-block scale terrain reaches from the whole grid's size.
#[test]
fn gridded_marker_art_samples_its_cell_at_source_resolution() {
    let page = TextureArray {
        layers: 1,
        mips: [64, 32, 16, 8, 4, 2, 1]
            .map(|size| TextureMip {
                size,
                rgba8: vec![255; (size * size * 4) as usize].into(),
            })
            .into(),
    };
    let texture = TextureRef::new(1, 0).unwrap();
    let overlay = assets::BlockOverlay {
        visuals: vec![assets::BlockVisual::diagnostic(
            assets::BlockFlags::empty(),
            assets::ContributorRole::Primary,
        )],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![Material {
            texture,
            ..Material::unvaried()
        }],
        textures: vec![page],
        texture_source_sizes: vec![[16; 2]],
        texture_source_grids: vec![1],
        ..Default::default()
    };
    let assets = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap();
    let marker = marker_texture(&assets, texture);
    assert_eq!(marker.uv_scale, 0.5);
    // Terrain: (16 << 1) / 64 * 0.5.
    assert_eq!(marker.gradient_scale, [0.25; 2]);
}

/// Every alternative and face that draws the same flipbook or texture shares one table entry
/// and its frames, so the bound tables grow with the pack's art, not with what is shown.
#[test]
fn alternatives_sharing_art_share_one_table_entry() {
    let encode = |texture: TextureRef| MarkerTexture {
        page: texture.page(),
        layer: texture.layer(),
        uv_scale: 1.0,
        reserved: 0,
        gradient_scale: [1.0; 2],
    };
    let frames = (0..64)
        .map(|layer| TextureRef::new(1, layer).unwrap())
        .collect::<Vec<_>>();
    let animations = [Animation {
        frame_start: 0,
        frame_count: 64,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags: 0,
    }];
    let mut tables = MarkerTables::default();
    for layer in 0..256 {
        let alternative = Material {
            texture: TextureRef::new(1, layer % 64).unwrap(),
            animation: 0,
            ..Material::unvaried()
        };
        assert_eq!(
            tables.push(Some(&alternative), &animations, &frames, encode),
            0
        );
    }
    let still = Material {
        texture: TextureRef::new(2, 3).unwrap(),
        ..Material::unvaried()
    };
    for _ in 0..6 {
        assert_eq!(tables.push(Some(&still), &animations, &frames, encode), 1);
        assert_eq!(tables.push(None, &animations, &frames, encode), 2);
    }
    assert_eq!(tables.animations.len(), 3);
    assert_eq!(tables.textures.len(), 65);
}
