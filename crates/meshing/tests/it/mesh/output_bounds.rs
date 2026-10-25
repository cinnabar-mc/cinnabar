/// Simple source models do not reserve output for unrelated complex asset templates.
#[test]
fn mesh_output_reserves_only_source_models() {
    let assets = runtime_assets();
    let bounds = meshing::MeshOutputBounds::new(assets);
    let simple = blocks(CROSS, &[[1, 2, 3]]);
    let compound = blocks(COMPOUND_40, &[[1, 2, 3]]);
    assert!(
        bounds.for_sub_chunk(&simple, assets, NetworkIdMode::Sequential)
            < bounds.for_sub_chunk(&compound, assets, NetworkIdMode::Sequential),
        "a two-quad plant must reserve less than a forty-quad compound model"
    );
}

/// Kelp changes visible quads within its fixed template, without selecting a larger model.
#[test]
fn mesh_output_fixed_flags_do_not_reserve_unrelated_models() {
    let assets = runtime_assets();
    let bounds = meshing::MeshOutputBounds::new(assets);
    assert!(
        bounds.for_sub_chunk(&uniform(KELP), assets, NetworkIdMode::Sequential)
            < bounds.for_sub_chunk(&uniform(COMPOUND_40), assets, NetworkIdMode::Sequential),
        "fixed kelp must reserve less than an unrelated forty-quad compound"
    );
}

/// Dense simple, compound, empty and neighbour-selected models fit their output credit.
#[test]
fn mesh_output_bounds_cover_dense_source_models() {
    let assets = runtime_assets();
    let bounds = meshing::MeshOutputBounds::new(assets);
    for id in [
        CROSS,
        ZERO_QUAD_CROSS,
        MODEL_32,
        COMPOUND_40,
        MIXED_ALPHA_MODEL,
        KELP,
    ] {
        let source = uniform(id);
        let mesh = mesh_sub_chunk(
            &classifier(),
            assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &source,
        );
        assert!(
            meshing::mesh_output_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
                <= bounds.for_sub_chunk(&source, assets, NetworkIdMode::Sequential),
            "source {id} exceeded its reservation"
        );
    }
}

/// Session compounds include displacement records in either network ID mode.
#[test]
fn mesh_output_bounds_cover_displaced_compounds_and_hashed_ids() {
    let base = runtime_assets();
    let id = base.visual_count() as u32;
    let hash = 0x2345_6789;
    let mut quad = base.model_quads()[0];
    quad.material = 0;
    let overlay = assets::BlockOverlay {
        visuals: vec![BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [0; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            support: assets::VisualSupport::VanillaFallback,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            flags: 0,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        }],
        model_templates: vec![
            ModelTemplate {
                quad_start: 0,
                quad_count: 24,
                flags: assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
            },
            ModelTemplate {
                quad_start: 24,
                quad_count: 16,
                flags: 0,
            },
        ],
        model_random_offsets: vec![
            (0, block_transform::random_offset::SHORT_GRASS),
            (1, block_transform::random_offset::SHORT_GRASS),
        ],
        model_quads: vec![quad; 40],
        textures: vec![base.texture_pages()[0].texture.clone()],
        hashes: vec![Some(hash)],
        ..assets::BlockOverlay::default()
    };
    let part_count = overlay.model_templates.len();
    let assets = base.with_block_overlay(id, &overlay).unwrap();
    let bounds = meshing::MeshOutputBounds::new(&assets);
    let mut sequential_bound = None;
    for (mode, value) in [
        (NetworkIdMode::Sequential, id),
        (NetworkIdMode::Hashed, hash),
    ] {
        let source = uniform(value);
        let mesh = mesh_sub_chunk(
            &classifier(),
            &assets,
            mode,
            &Neighbourhood::empty(),
            &source,
        );
        assert_eq!(
            mesh.model_refs().len(),
            world::BLOCKS_PER_SUB_CHUNK * part_count
        );
        assert!(mesh.model_refs().iter().all(|reference| {
            reference.words()[0] & meshing::MODEL_REF_FLAG_RANDOM_OFFSET != 0
        }));
        let reserved = bounds.for_sub_chunk(&source, &assets, mode);
        assert!(
            meshing::mesh_output_byte_len(&mesh, &meshing::PackedBiomeRecord::fallback())
                <= reserved
        );
        assert_eq!(*sequential_bound.get_or_insert(reserved), reserved);
    }
}
