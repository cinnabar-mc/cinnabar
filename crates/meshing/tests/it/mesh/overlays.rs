/// Air (0), a full opaque cube host (1) and a plate overlay state (2): one quad, the north face of
/// a box authored at `z` 0 to 2 pixels, drawn with an opaque material.
fn overlay_fixture() -> RuntimeAssets {
    let mut air = BlockVisual::diagnostic(BlockFlags::AIR, assets::ContributorRole::Air);
    air.kind = VisualKind::Invisible;
    air.support = assets::VisualSupport::Exact;
    let mut host = BlockVisual::diagnostic(
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        assets::ContributorRole::Primary,
    );
    host.kind = VisualKind::Cube;
    host.support = assets::VisualSupport::Exact;
    host.faces = [0; Face::ALL.len()];
    let mut plate = host;
    plate.kind = VisualKind::Model;
    plate.flags = BlockFlags::empty();
    plate.model_template = 0;
    let visuals = vec![air, host, plate].into_boxed_slice();
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::default(); visuals.len()]
            .into_boxed_slice(),
        hashed: (0..visuals.len() as u32)
            .map(|id| (0x10000 + id, id))
            .collect(),
        visuals,
        materials: vec![Material::unvaried()].into_boxed_slice(),
        model_templates: vec![ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }]
        .into_boxed_slice(),
        model_quads: vec![ModelQuad {
            positions: [[256, 0, 0], [0, 0, 0], [0, 256, 0], [256, 256, 0]],
            uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            material: 0,
            flags: 5,
        }]
        .into_boxed_slice(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![255; (size * size * 4) as usize].into_boxed_slice(),
                })
                .collect(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA6; 32],
            block_registry_sha256: [0x6A; 32],
            light_registry_sha256: [0x34; 32],
            biome_registry_sha256: [0x3D; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

/// The overlay fixture's host at `cell`, in `mode`'s network values.
fn overlay_host_chunk(mode: NetworkIdMode, cell: [u8; 3]) -> SubChunk {
    let palette = (0..3)
        .map(|id| if mode == NetworkIdMode::Hashed { 0x10000 + id } else { id })
        .collect::<Vec<_>>();
    let mut data = vec![9, 1, 0];
    data.extend(packed_storage(4, &palette, &[(cell, 1)]));
    SubChunk::decode(&data, &RawBlockIds { air: palette[0] })
}

/// Meshes the overlay fixture with `overlays` on its host.
fn mesh_overlays(mode: NetworkIdMode, cell: [u8; 3], overlays: &[meshing::CellOverlay]) -> ChunkMesh {
    let air = if mode == NetworkIdMode::Hashed { 0x10000 } else { 0 };
    let chunk = overlay_host_chunk(mode, cell);
    meshing::mesh_sub_chunk_in_neighbourhood_with_options(
        &BlockClassifier::new(air),
        &overlay_fixture(),
        mode,
        &MeshNeighbourhood::new(&chunk),
        &meshing::FullBrightLightSampler,
        meshing::MeshOptions {
            overlays,
            ..Default::default()
        },
    )
}

#[test]
fn an_overlay_authored_against_north_lands_against_each_face() {
    // Bedrock faces and the axis and side of the cell each lies on.
    let planes = [(1, 0), (1, 256), (2, 0), (2, 256), (0, 0), (0, 256)];
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let plate = if mode == NetworkIdMode::Hashed { 0x10002 } else { 2 };
        let cell = [5, 9, 12];
        let overlays: Vec<_> = (0..6)
            .map(|face| meshing::CellOverlay {
                local: cell,
                face,
                network_value: plate,
            })
            .collect();
        let mesh = mesh_overlays(mode, cell, &overlays);
        assert_eq!(mesh.model_refs().len(), 6, "{mode:?}: one reference per overlay");
        assert_eq!(mesh.model_lighting().len(), 6);
        assert_eq!(mesh.model_draw_refs().len(), 6);
        let quad = overlay_fixture().model_quads()[0];
        for (face, reference) in mesh.model_refs().iter().enumerate() {
            let [transform, template, ..] = reference.words();
            assert_eq!(template, 0);
            assert_eq!(
                [transform & 15, (transform >> 4) & 15, (transform >> 8) & 15],
                cell.map(u32::from)
            );
            let turn = transform >> 12;
            assert_eq!(Some(turn), meshing::overlay_model_turn(face as u8));
            let (axis, side) = planes[face];
            for corner in quad.positions {
                assert_eq!(
                    meshing::turn_model_position(corner, turn)[axis],
                    side,
                    "{mode:?}: face {face} corner {corner:?}"
                );
            }
        }
    }
}

#[test]
fn overlays_add_nothing_to_the_host_and_cost_no_cube_faces() {
    let cell = [8, 8, 8];
    let bare = mesh_overlays(NetworkIdMode::Sequential, cell, &[]);
    let dressed = mesh_overlays(
        NetworkIdMode::Sequential,
        cell,
        &[meshing::CellOverlay {
            local: cell,
            face: 2,
            network_value: 2,
        }],
    );
    assert!(bare.model_refs().is_empty());
    assert_eq!(dressed.cube_quads(), bare.cube_quads(), "the host meshes alone");
    assert_eq!(dressed.model_refs().len(), 1);
    let unknown = mesh_overlays(
        NetworkIdMode::Sequential,
        cell,
        &[meshing::CellOverlay {
            local: cell,
            face: 2,
            network_value: 1,
        }],
    );
    assert!(unknown.model_refs().is_empty(), "a cube state draws no overlay");
}
