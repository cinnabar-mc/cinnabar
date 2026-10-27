use super::*;

#[test]
fn any_raster_drawn_with_a_colour_mask_material_keeps_its_mask_alpha() {
    assert_eq!(probe_alpha(3, "sheep"), (1, 3));
}

#[test]
fn vanilla_sheep_rasters_and_all_geometry_bindings_retain_native_color_mask_alpha() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT").map(std::path::PathBuf::from)
    else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ACTOR_ROOT is not set");
        return;
    };
    if !root.exists() {
        eprintln!(
            "skipping missing fixture: CINNABAR_VANILLA_ACTOR_ROOT at {}",
            root.display()
        );
        return;
    }

    let root = Path::new(&root);
    if !root.join("entity/sheep.entity.json").is_file() {
        eprintln!(
            "missing fixture: pinned vanilla sheep entity at {}",
            root.display()
        );
        return;
    }
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let entity_bytes = encode_entity_blob(&entities).unwrap();
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap(),
    )
    .unwrap();
    let sheep_rigs: Vec<_> = entities
        .rig_bindings
        .iter()
        .enumerate()
        .filter(|(_, rig)| {
            entities.symbols[rig.entity_symbol as usize]
                .identifier
                .as_ref()
                == "minecraft:sheep"
        })
        .collect();
    assert!(!sheep_rigs.is_empty());
    for (rig_index, rig) in sheep_rigs {
        let count = catalog
            .bindings()
            .iter()
            .filter(|binding| binding.rig as usize == rig_index)
            .count();
        assert_eq!(count, usize::from(rig.geometry_count));
        assert!(
            !compiled
                .report
                .fallbacks
                .iter()
                .any(|entry| entry.rig as usize == rig_index)
        );
    }
    let mut masks = 0;
    for texture in catalog.textures() {
        let source = &entities.sources[texture.source as usize];
        if !matches!(
            source.path.as_ref(),
            "textures/entity/sheep/sheep.tga" | "textures/entity/sheep/sheep_baby.tga"
        ) {
            continue;
        }
        masks += 1;
        let raster = image::open(root.join(source.path.as_ref()))
            .unwrap()
            .into_rgba8()
            .into_raw();
        assert_eq!(
            texture.rgba8.as_ref(),
            raster,
            "mask alpha is not quantized"
        );
        assert!(
            raster
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| !matches!(pixel[3], 0 | 255))
        );
    }
    assert_eq!(
        masks, 2,
        "both adult and baby rasters keep their mask alpha"
    );
}
