use super::*;

const SHEET: u32 = 1;

/// A plan for material [`SHEET`] with one solid chain per quadrant.
fn plan() -> SheetPlan {
    let chain = || {
        let base = [80, 100, 120, 255].repeat((TILE_SIZE * TILE_SIZE) as usize);
        assets::build_texture_mip_chain(base.into_boxed_slice(), TILE_SIZE).unwrap()
    };
    SheetPlan {
        paths: BTreeMap::from([(SHEET, "sheet".into())]),
        chains: BTreeMap::from([("sheet".into(), std::array::from_fn(|_| chain()))]),
        ..SheetPlan::default()
    }
}

/// A page with `layers` copies of one canonical chain.
fn page(layers: usize) -> TexturePage {
    let base = [10, 20, 30, 255].repeat((TILE_SIZE * TILE_SIZE) as usize);
    let chain = assets::build_texture_mip_chain(base.into_boxed_slice(), TILE_SIZE).unwrap();
    TexturePage::new(TextureArray {
        layers: layers as u32,
        mips: chain
            .iter()
            .map(|mip| TextureMip {
                size: mip.size,
                rgba8: mip.rgba8.repeat(layers).into_boxed_slice(),
            })
            .collect(),
    })
}

/// A drawn shelf pair whose one quad samples the whole sheet's lower right quadrant.
fn model() -> (Vec<BlockVisual>, Vec<ModelTemplate>, Vec<ModelQuad>) {
    let visual = BlockVisual {
        kind: VisualKind::Model,
        model_template: 0,
        ..BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
    };
    let quad = ModelQuad {
        positions: [[0; 3]; 4],
        uvs: [[2058, 2058], [2058, 4086], [4086, 4086], [4086, 2058]],
        material: SHEET,
        flags: 0,
    };
    let template = |quad_start| ModelTemplate {
        quad_start,
        quad_count: 1,
        flags: MODEL_TEMPLATE_FLAG_SHELF,
    };
    (vec![visual], vec![template(0), template(1)], vec![quad; 2])
}

#[test]
fn installed_tiles_rewrite_only_the_first_template_of_a_pair() {
    let (visuals, mut templates, mut quads) = model();
    let mut materials = vec![Material::unvaried(); 2];
    materials[SHEET as usize].flags = MATERIAL_FLAG_ALPHA_CUTOUT;
    let mut pages = vec![page(1)];
    let sheet_quad = quads[1];
    plan()
        .install(
            &mut materials,
            &mut pages,
            &visuals,
            &mut templates,
            &mut quads,
        )
        .unwrap();
    assert_eq!(pages[0].texture.layers, 5);
    assert_eq!(materials.len(), 6);
    assert_eq!(
        templates[0].flags,
        MODEL_TEMPLATE_FLAG_SHELF | MODEL_TEMPLATE_FLAG_SHEET_TILES
    );
    assert_eq!(templates[1].flags, MODEL_TEMPLATE_FLAG_SHELF);
    assert_eq!(quads[1], sheet_quad);
    assert_eq!(quads[0].material, 5, "the lower right quadrant");
    assert_eq!(quads[0].uvs[0], [20, 20]);
    assert_eq!(quads[0].uvs[2], [4076, 4076]);
    assert_eq!(materials[5].flags, MATERIAL_FLAG_ALPHA_CUTOUT);
}

/// Texture copies every pack needs run first, so optional layers cannot take their room.
#[test]
fn a_carrier_without_room_keeps_its_whole_sheet_templates() {
    let (visuals, mut templates, mut quads) = model();
    let mut materials = vec![Material::unvaried(); 2];
    let mut pages = vec![
        page(MAX_CARRIER_TEXTURE_LAYERS),
        page(MAX_CARRIER_TEXTURE_LAYERS - QUADRANTS + 1),
    ];
    let (original_pages, original_quads) = (pages.clone(), quads.clone());
    plan()
        .install(
            &mut materials,
            &mut pages,
            &visuals,
            &mut templates,
            &mut quads,
        )
        .unwrap();
    assert_eq!(pages, original_pages);
    assert_eq!(quads, original_quads);
    assert_eq!(materials.len(), 2);
    assert!(
        templates
            .iter()
            .all(|template| template.flags == MODEL_TEMPLATE_FLAG_SHELF)
    );
}

/// A further base page would take a texture array from every session overlay.
#[test]
fn optional_layers_never_open_a_new_page() {
    let (visuals, mut templates, mut quads) = model();
    let mut materials = vec![Material::unvaried(); 2];
    let mut pages = vec![page(MAX_CARRIER_TEXTURE_LAYERS - QUADRANTS + 1)];
    let original = pages.clone();
    plan()
        .install(
            &mut materials,
            &mut pages,
            &visuals,
            &mut templates,
            &mut quads,
        )
        .unwrap();
    assert_eq!(pages, original);
    assert_eq!(templates[0].flags, MODEL_TEMPLATE_FLAG_SHELF);
}

#[test]
fn the_last_exact_room_is_used() {
    let (visuals, mut templates, mut quads) = model();
    let mut materials = vec![Material::unvaried(); 2];
    let mut pages = vec![
        page(MAX_CARRIER_TEXTURE_LAYERS),
        page(MAX_CARRIER_TEXTURE_LAYERS - QUADRANTS),
    ];
    plan()
        .install(
            &mut materials,
            &mut pages,
            &visuals,
            &mut templates,
            &mut quads,
        )
        .unwrap();
    assert_eq!(pages[1].texture.layers as usize, MAX_CARRIER_TEXTURE_LAYERS);
    assert_ne!(templates[0].flags & MODEL_TEMPLATE_FLAG_SHEET_TILES, 0);
}

#[test]
fn undrawn_templates_and_other_families_stay_untouched() {
    let (mut visuals, mut templates, mut quads) = model();
    visuals[0].model_template = 1;
    templates[0].flags = 0;
    templates[1].flags = 0;
    let original = (templates.clone(), quads.clone());
    let mut materials = vec![Material::unvaried(); 2];
    let mut pages = vec![page(1)];
    plan()
        .install(
            &mut materials,
            &mut pages,
            &visuals,
            &mut templates,
            &mut quads,
        )
        .unwrap();
    assert_eq!((templates, quads), original);
}
