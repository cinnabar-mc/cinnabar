#[test]
fn reused_mesh_scratch_preserves_every_stream_after_large_and_layered_sections() {
    let large = (0..16_u8)
        .flat_map(|x| (0..16_u8).flat_map(move |y| (0..16_u8).map(move |z| [x, y, z])))
        .filter(|[x, y, z]| (x + y + z) % 2 == 0)
        .map(|cell| (cell, 1))
        .collect::<Vec<_>>();
    let sections = [
        sub_chunk(vec![packed_storage(1, &[AIR, OPAQUE_A], &large)]),
        sub_chunk(vec![
            packed_storage(
                3,
                &[AIR, OPAQUE_A, CROSS, MIXED_ALPHA_MODEL],
                &[([1, 2, 3], 1), ([5, 6, 7], 2), ([9, 10, 11], 3)],
            ),
            packed_storage(1, &[AIR, LIQUID_A], &[([5, 6, 7], 1), ([2, 3, 4], 1)]),
        ]),
        uniform(AIR),
        uniform(OPAQUE_B),
        blocks(DIAGNOSTIC, &[[4, 5, 6]]),
        blocks(LEAF_A, &[[0, 1, 2], [0, 2, 2]]),
        sub_chunk(vec![packed_storage(
            16,
            &[AIR, OPAQUE_B],
            &[([3, 4, 5], 1)],
        )]),
    ];
    let changed_assets = layout_fixture_assets();
    let mut scratch = meshing::MeshScratch::default();
    let mut retained = Vec::new();
    let sampler = |[x, y, z]: [i32; 3]| {
        MeshLightSample::try_new(x.rem_euclid(16) as u8, (y + z).rem_euclid(16) as u8).unwrap()
    };
    for assets in [runtime_assets(), &changed_assets, runtime_assets()] {
        for (index, section) in sections.iter().enumerate() {
            let mut neighbourhood = MeshNeighbourhood::new(section);
            neighbourhood.insert([1, 0, 0], &sections[3]);
            let options = meshing::MeshOptions {
                split_tinted_faces: true,
                ..Default::default()
            };
            let fresh = meshing::mesh_sub_chunk_in_neighbourhood_with_options(
                &classifier(),
                assets,
                NetworkIdMode::Sequential,
                &neighbourhood,
                &sampler,
                options,
            );
            let reused = meshing::mesh_sub_chunk_in_neighbourhood_with_scratch(
                &classifier(),
                assets,
                NetworkIdMode::Sequential,
                &neighbourhood,
                &sampler,
                options,
                &mut scratch,
            );
            assert_eq!(reused, fresh);
            if std::ptr::eq(assets, runtime_assets()) && index == 1 {
                assert!(!reused.cube_quads().is_empty());
                assert!(!reused.model_refs().is_empty());
                assert!(!reused.model_draw_refs().is_empty());
                assert!(!reused.transparent_model_draw_refs().is_empty());
            }
            retained.push((reused, fresh));
        }
    }
    drop(scratch);
    for (reused, fresh) in retained {
        assert_eq!(
            reused, fresh,
            "later jobs and scratch release changed a published mesh"
        );
    }
}

#[test]
fn reused_mesh_scratch_resolves_hashed_palettes_and_overlay_options_again() {
    let mut scratch = meshing::MeshScratch::default();
    let assets = overlay_fixture();
    for mode in [
        NetworkIdMode::Sequential,
        NetworkIdMode::Hashed,
        NetworkIdMode::Sequential,
    ] {
        let air = if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        };
        let section = overlay_host_chunk(mode, [2, 3, 4]);
        let overlay = meshing::CellOverlay {
            local: [2, 3, 4],
            network_value: air + 2,
            face: 2,
        };
        for overlays in [&[overlay][..], &[][..]] {
            let options = meshing::MeshOptions {
                overlays,
                ..Default::default()
            };
            let neighbourhood = MeshNeighbourhood::new(&section);
            let fresh = meshing::mesh_sub_chunk_in_neighbourhood_with_options(
                &BlockClassifier::new(air),
                &assets,
                mode,
                &neighbourhood,
                &meshing::FullBrightLightSampler,
                options,
            );
            let reused = meshing::mesh_sub_chunk_in_neighbourhood_with_scratch(
                &BlockClassifier::new(air),
                &assets,
                mode,
                &neighbourhood,
                &meshing::FullBrightLightSampler,
                options,
                &mut scratch,
            );
            assert_eq!(reused, fresh);
        }
    }
}
