use super::*;
use pack_runtime::reload_diff::{Changes, Subscriber};

#[test]
fn first_reload_skips_unused_file_reads() {
    use resource_pack::test_support::file_read_count;
    for enable_globals in [false, true] {
        let server = super::super::pack_reload_tests::stack(&[(
            "extras/unused.bin",
            b"unconsumed server payload",
        )]);
        let globals = super::super::pack_reload_tests::stack(&[(
            "extras/unused.bin",
            b"unconsumed global payload",
        )]);
        let joined = resource_packs::prepare_validated_application(server.clone(), Arc::default());
        assert_eq!(
            file_read_count(&server.packs()[0], "extras/unused.bin"),
            Some(0)
        );
        let mut reload = PackReload::default();
        if enable_globals {
            reload.request_globals(globals.clone());
        }
        reload.begin_session(1, &joined);
        reload.start(
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            Some(EnvironmentBase::new(
                Default::default(),
                None,
                Default::default(),
            )),
            None,
            None,
        );
        let prepared = reload
            .pending
            .as_ref()
            .unwrap()
            .result
            .lock()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        for source in [&server, &globals] {
            assert_eq!(
                file_read_count(&source.packs()[0], "extras/unused.bin"),
                Some(0)
            );
        }
        for subscriber in [Subscriber::Atmosphere, Subscriber::Particles] {
            assert!(prepared.application.dependencies.contains_key(&subscriber));
        }
    }
}

#[test]
fn global_packs_only_recompile_subscribers_with_changed_reads() {
    let server = super::super::pack_reload_tests::stack(&[("texts/en_US.lang", b"server=value")]);
    let joined = resource_packs::prepare_validated_application(server, Arc::default());
    let globals = super::super::pack_reload_tests::stack(&[
        ("texts/en_US.lang", b"global=value"),
        ("extras/global.txt", b"global-only"),
    ]);
    let mut reload = PackReload::default();
    reload.request_globals(globals.clone());
    reload.begin_session(1, &joined);
    let composed = ValidatedPackStack::compose(&globals, &reload.server).unwrap();
    let changes = Changes::between(
        &composed,
        reload
            .previous
            .as_ref()
            .map(|old| (&old.admission, old.dependencies.as_ref())),
    );
    let recompiled: Vec<_> = [
        (Subscriber::Blocks, changes.blocks),
        (Subscriber::BlockEntityArt, changes.block_entity_art),
        (Subscriber::Icons, changes.icons),
        (Subscriber::Glyphs, changes.glyphs),
        (Subscriber::Entities, changes.entities),
        (Subscriber::Ui, changes.ui),
        (Subscriber::Sounds, changes.sounds),
        (Subscriber::Language, changes.language),
        (Subscriber::AimAssist, changes.aim_assist),
    ]
    .into_iter()
    .filter_map(|(subscriber, changed)| changed.then_some(subscriber))
    .collect();
    assert_eq!(recompiled, [Subscriber::Sounds, Subscriber::Language]);
    let composed = Arc::new(composed);
    let reused = resource_packs::prepare_diffed_application(
        reload.kept,
        composed.clone(),
        joined.inputs.clone(),
        reload.previous.as_ref().map(|old| (old, changes)),
    );
    let fresh = resource_packs::prepare_validated_application(composed, joined.inputs.clone());
    for key in ["server", "global"] {
        assert_eq!(
            reused
                .server_lang
                .as_ref()
                .and_then(|lang| lang.lookup(key)),
            fresh.server_lang.as_ref().and_then(|lang| lang.lookup(key))
        );
    }
    assert_eq!(reused.terrain_identity, fresh.terrain_identity);
    assert_eq!(reused.dependencies, fresh.dependencies);
}

#[test]
fn first_reload_still_prepares_biomes_and_environment() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(
        assets::TINT_MAP_SIZE,
        assets::TINT_MAP_SIZE,
        image::Rgb([12, 34, 56]),
    )
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let path = format!(
        "textures/colormap/{}.png",
        assets::TintMapId::Grass.source_name()
    );
    let stack = super::super::pack_reload_tests::stack(&[(&path, png.get_ref())]);
    let joined = resource_packs::prepare_validated_application(stack, Arc::default());
    let diagnostic = assets::RuntimeAssets::diagnostic();
    let carrier = assets::CompiledAssets {
        visuals: vec![assets::BlockVisual::diagnostic(
            assets::BlockFlags::empty(),
            assets::ContributorRole::Primary,
        )]
        .into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK].into(),
        hashed: Box::new([]),
        materials: diagnostic.materials().into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![diagnostic.texture_pages()[0].clone(); 2].into(),
        biomes: diagnostic.biome_assets().clone(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    let base =
        Arc::new(assets::RuntimeAssets::decode(&assets::encode_blob(&carrier).unwrap()).unwrap());
    let mut reload = PackReload {
        previous_assets: Some(base.clone()),
        ..Default::default()
    };
    reload.begin_session(1, &joined);
    assert!(
        reload.previous_assets.is_none(),
        "a menu carrier cannot stand in for the join"
    );
    reload.start(
        base,
        None,
        Some(EnvironmentBase::new(
            Default::default(),
            None,
            Default::default(),
        )),
        None,
        None,
    );
    let prepared = reload
        .pending
        .as_ref()
        .unwrap()
        .result
        .lock()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(
        &prepared.assets.biome_assets().tint_maps_rgb8[..3],
        &[12, 34, 56]
    );
    for subscriber in [Subscriber::Atmosphere, Subscriber::Particles] {
        assert!(prepared.application.dependencies.contains_key(&subscriber));
    }
    assert!(prepared.application.dependencies[&Subscriber::Blocks].iter().any(|dependency| {
        matches!(dependency, resource_pack::PackDependency::File { path: read, .. } if read == &path)
    }));
}

#[test]
fn graphics_mode_reload_preserves_biome_colors_and_dependencies() {
    let diagnostic = assets::RuntimeAssets::diagnostic();
    let biomes = assets::CompiledBiomeAssets {
        rules: vec![assets::BiomeRule {
            id: 0,
            name: "test:biome".into(),
            flags: 0,
            grass: assets::TintSource::map(assets::TintMapId::Grass),
            foliage: assets::TintSource::map(assets::TintMapId::Foliage),
            dry_foliage: assets::TintSource::map(assets::TintMapId::DryFoliage),
            water: assets::TintSource::direct(0xffffff),
            temperature_bits: 0.5_f32.to_bits(),
            downfall_bits: 0.5_f32.to_bits(),
        }]
        .into(),
        ..diagnostic.biome_assets().clone()
    };
    let base = Arc::new(
        diagnostic
            .with_block_overlay(
                diagnostic.visual_count() as u32,
                &assets::BlockOverlay {
                    biomes: Some(biomes),
                    ..Default::default()
                },
            )
            .unwrap(),
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(
        assets::TINT_MAP_SIZE,
        assets::TINT_MAP_SIZE,
        image::Rgb([12, 34, 56]),
    )
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let path = format!(
        "textures/colormap/{}.png",
        assets::TintMapId::Grass.source_name()
    );
    let biome = serde_json::to_vec(&serde_json::json!({"minecraft:client_biome": {
        "description": {"identifier": base.biome_assets().rules[0].name.as_ref()},
        "components": {"minecraft:water_appearance": {"surface_color": "#123456"}}
    }}))
    .unwrap();
    let stack = super::super::pack_reload_tests::stack(&[
        (&path, png.get_ref()),
        ("biomes/test.json", &biome),
    ]);
    let joined = resource_packs::prepare_validated_application(stack, Arc::default());
    let mut reload = PackReload::default();
    reload.begin_session(1, &joined);
    let first = prepare_reload(&mut reload, &base);
    assert_eq!(
        &first.assets.biome_assets().tint_maps_rgb8[..3],
        &[12, 34, 56]
    );
    assert_eq!(
        first.assets.biome_assets().rules[0].water,
        assets::TintSource::direct(0x123456)
    );

    for graphics_changed in [false, true] {
        let mut previous = first.application.clone();
        if graphics_changed {
            previous.fancy_graphics = !previous.fancy_graphics;
        }
        reload.previous = Some(previous);
        reload.previous_assets = Some(first.assets.clone());
        reload.request_globals(super::super::pack_reload_tests::stack(&[(
            "texts/en_US.lang",
            b"global=value",
        )]));
        let composed = ValidatedPackStack::compose(&reload.globals, &reload.server).unwrap();
        let changes = Changes::between(
            &composed,
            Some((
                &first.application.admission,
                first.application.dependencies.as_ref(),
            )),
        );
        assert!(changes.language);
        assert!(!changes.blocks, "the reload changes only language inputs");

        let next = prepare_reload(&mut reload, &base);
        assert_eq!(next.assets.biome_assets(), first.assets.biome_assets());
        assert_eq!(
            next.application.dependencies[&Subscriber::Blocks],
            first.application.dependencies[&Subscriber::Blocks]
        );
        assert_eq!(
            next.application.terrain_identity,
            first.application.terrain_identity
        );
        assert_eq!(
            next.application
                .server_lang
                .as_ref()
                .unwrap()
                .lookup("global"),
            Some("value")
        );
        if !graphics_changed {
            assert!(Arc::ptr_eq(&next.assets, &first.assets));
        }
    }
}

/// Runs one reload worker to completion without publishing its result to a frame.
fn prepare_reload(reload: &mut PackReload, base: &Arc<assets::RuntimeAssets>) -> Prepared {
    reload.start(base.clone(), None, None, None, None);
    reload
        .pending
        .take()
        .unwrap()
        .result
        .into_inner()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap()
}

/// Frame publication keeps dependency evidence shared instead of cloning every recorded read.
#[test]
fn pack_application_clones_do_no_dependency_sized_work() {
    for count in [16, 16_384] {
        let reads = (0..count)
            .map(|index| resource_pack::PackDependency::File {
                path: format!("textures/fixture/{index}.png"),
                limit: 64,
            })
            .collect();
        let application = PackApplication {
            dependencies: pack_runtime::reload_diff::Dependencies::from([(
                Subscriber::Icons,
                reads,
            )])
            .into(),
            ..Default::default()
        };
        let (copy, allocations) = test_alloc::measure(|| application.clone());
        assert_eq!(copy.dependencies, application.dependencies);
        assert!(
            allocations.calls <= 32 && allocations.bytes <= 16 * 1024,
            "frame copied {count} dependency records: {allocations:?}"
        );
    }
}
