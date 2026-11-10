use super::page_budget::within_page_budget;
use super::*;
use crate::MAX_RENDERED_PLAYERS;

/// Route-only changes can retain the pixel identity but must invalidate prepared artwork.
#[test]
fn shared_artwork_snapshot_checks_routes_as_well_as_pixels() {
    let original = ActorArtworkPages::default();
    let mut changed = original.clone();
    assert!(original.shares_storage_with(&changed));
    Arc::make_mut(&mut changed.routes).insert(
        EntityRigId(0),
        ActorArtworkLocation {
            page: 1,
            layer: 0,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        },
    );
    assert_eq!(original.identity(), changed.identity());
    assert!(Arc::ptr_eq(&original.pages, &changed.pages));
    assert!(!original.shares_storage_with(&changed));
}

#[test]
fn page_budget_reserves_player_capacity_and_checks_exact_boundaries() {
    assert_eq!(MAX_RENDERED_PLAYERS, 128);
    assert!(player_page_bytes() < MAX_ACTOR_GPU_PIXEL_BYTES);
    assert_eq!(assets::MAX_ACTOR_TEXTURES, 2048);
    assert!(within_page_budget(
        MAX_ACTOR_TEXTURE_PAGES - 1,
        MAX_ACTOR_GPU_PIXEL_BYTES
    ));
    assert!(!within_page_budget(
        MAX_ACTOR_TEXTURE_PAGES,
        MAX_ACTOR_GPU_PIXEL_BYTES
    ));
    assert!(!within_page_budget(
        MAX_ACTOR_TEXTURE_PAGES - 1,
        MAX_ACTOR_GPU_PIXEL_BYTES + 1
    ));
}

#[test]
fn base_source_override_retargets_variants_and_releases_replaced_pixels() {
    // Keep common pack pixels live across the override's release assertion.
    let unrelated = ActorArtworkPages::default().with_pack_artwork(
        &[assets::ActorTexture {
            source: 0,
            width: 2,
            height: 2,
            pixel_sha256: Sha256::digest([7; 16]).into(),
            rgba8: Arc::from([7; 16]),
        }],
        &[],
    );
    let route = ActorArtworkLocation {
        page: 1,
        layer: 0,
        pose_mode: assets::ActorPoseMode::CompiledLiteral,
        multitexture: None,
    };
    let base = ActorArtworkPages {
        pages: vec![ActorTexturePage {
            width: 1,
            height: 1,
            layers: 1,
            rgba8: vec![3; 4].into(),
            color_mask: false,
            multitexture: false,
        }]
        .into(),
        routes: Arc::new(BTreeMap::from([(EntityRigId(0), route)])),
        source_locations: Arc::new(BTreeMap::from([(5, route)])),
        entity_locations: Arc::new(BTreeSet::from([(1, 0)])),
        ..Default::default()
    };
    // Distinct bytes give the lifetime assertion its own allocation.
    let replacement_pixels = *b"override pixels!";
    let replacement = EquipmentRaster {
        width: 2,
        height: 2,
        rgba8: replacement_pixels.into(),
    };
    let applied = base
        .clone()
        .with_source_texture_overrides(&[(5, replacement)]);
    let new_route = applied.variant_location(EntityRigId(0), 5).unwrap();
    assert_eq!(applied.route(EntityRigId(0)), Some(new_route));
    assert_ne!(new_route.page, route.page);
    let page = &applied.pages()[usize::from(new_route.page) - 1];
    assert_eq!((page.width, page.height), (2, 2));
    assert_eq!(page.pixels().as_ref(), &replacement_pixels);
    let pixels = page.rgba8.downgrade();
    drop(applied);
    assert!(pixels.iter().all(|pixels| pixels.upgrade().is_none()));
    assert_eq!(base.route(EntityRigId(0)), Some(route));
    assert_eq!(unrelated.pages()[0].pixels().as_ref(), &[7; 16]);
}

#[test]
/// Different pixel and alpha values stay in their assigned texture layers.
fn packed_artwork_preserves_layer_pixels_and_equipment_locations() {
    let first: Arc<[u8]> = Arc::from([1, 2, 3, 4]);
    let second: Arc<[u8]> = Arc::from([5, 6, 7, 8]);
    let textures = [first.clone(), second.clone()].map(|rgba8| assets::ActorTexture {
        source: 0,
        width: 1,
        height: 1,
        pixel_sha256: [0; 32],
        rgba8,
    });
    let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
    assert_eq!(
        pages.pages()[0].pixels().as_ref(),
        &[1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(pages.pages()[0].layers(), 2);
    let rasters = [first, second].map(|rgba8| EquipmentRaster {
        width: 1,
        height: 1,
        rgba8,
    });
    let (equipment, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
    assert_eq!(
        equipment.pages()[0].pixels().as_ref(),
        &[1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(locations[0].unwrap().layer(), 0);
    assert_eq!(locations[1].unwrap().layer(), 1);
    assert_eq!(locations[0].unwrap().page(), locations[1].unwrap().page());
    assert_eq!(pages.identity(), equipment.identity());
}

#[test]
fn fitting_a_page_halves_until_it_fits_and_averages_texels() {
    let page = ActorTexturePage {
        width: 2,
        height: 6,
        layers: 1,
        rgba8: [[0u8, 0, 0, 255], [255, 255, 255, 255]]
            .repeat(6)
            .concat()
            .into(),
        color_mask: true,
        multitexture: false,
    };
    let fitted = page.fit_within(4);
    assert_eq!((fitted.width, fitted.height), (1, 3));
    assert_eq!(&fitted.pixels()[..4], &[127, 127, 127, 255]);
    assert!(
        fitted.color_mask,
        "downscaling retains the material contract"
    );
    assert!(matches!(page.fit_within(6), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn pack_artwork_routes_bindings_under_pack_rig_ids() {
    let texture = |side: u16| assets::ActorTexture {
        source: 0,
        width: side,
        height: side,
        pixel_sha256: [1; 32],
        rgba8: vec![7; usize::from(side) * usize::from(side) * 4].into(),
    };
    let binding = |candidate: u32, texture: u32| assets::ActorArtworkBinding {
        rig: 0,
        geometry_candidate: candidate,
        entity_symbol: 0,
        geometry: 0,
        render_controller: 0,
        texture,
        material: "entity".into(),
        pose_mode: assets::ActorPoseMode::CompiledLiteral,
    };
    let pages = ActorArtworkPages::default()
        .with_pack_artwork(&[texture(2)], &[binding(5, 0), binding(6, 9)]);
    assert_eq!(pages.pages().len(), 1);
    let location = pages.route(crate::pack_rig_id(5)).unwrap();
    assert!(pages.valid(crate::pack_rig_id(5), location));
    assert_eq!(pages.route(crate::pack_rig_id(6)), None);
    assert!(!crate::is_equipment_rig_id(crate::pack_rig_id(5)));
    assert!(crate::is_pack_rig_id(crate::pack_rig_id(5)));
    assert_eq!(pages.rejected_bindings(), 1);
    assert_ne!(pages.identity(), [0; 32]);
}

// A player's controller may name a catalog texture beside the skin it wears, as a pack's
// tracer layer does; its skin model draws that texture although it has no route.
#[test]
fn skin_models_draw_catalog_textures_their_controllers_name() {
    let texture = |source: u32, fill: u8| assets::ActorTexture {
        source,
        width: 2,
        height: 2,
        pixel_sha256: [fill; 32],
        rgba8: vec![fill; 16].into(),
    };
    let pages = ActorArtworkPages::default().with_pack_artwork(&[texture(23, 1)], &[]);
    let skin = crate::skin_rig_id(3);
    assert!(crate::is_skin_rig_id(skin));
    assert!(!crate::is_skin_rig_id(crate::item_mesh_rig_id(3)));
    assert!(!crate::is_skin_rig_id(crate::DIAGNOSTIC_RIG_ID));
    assert!(!crate::is_skin_rig_id(crate::actor::flame::FLAME_RIG_ID));
    let location = pages.source_location(true, 23).unwrap();
    assert!(pages.valid(skin, location));
    assert_eq!(
        pages.source_location(false, 23),
        None,
        "vanilla sources stay separate"
    );
    let unknown = ActorArtworkLocation {
        page: location.page,
        layer: location.layer + 1,
        ..location
    };
    assert!(!pages.valid(skin, unknown));
}

// Pack render layers name pack-catalog sources, never vanilla ones.
#[test]
fn pack_rig_variants_resolve_in_the_pack_source_space() {
    let texture = |source: u32, fill: u8| assets::ActorTexture {
        source,
        width: 2,
        height: 2,
        pixel_sha256: [fill; 32],
        rgba8: vec![fill; 16].into(),
    };
    let binding = assets::ActorArtworkBinding {
        rig: 0,
        geometry_candidate: 0,
        entity_symbol: 0,
        geometry: 0,
        render_controller: 0,
        texture: 0,
        material: "entity".into(),
        pose_mode: assets::ActorPoseMode::CompiledLiteral,
    };
    let pages =
        ActorArtworkPages::default().with_pack_artwork(&[texture(4, 1), texture(9, 2)], &[binding]);
    let rig = crate::pack_rig_id(0);
    let variant = pages.variant_location(rig, 9).unwrap();
    assert_eq!((variant.page(), variant.layer()), (1, 1));
    assert!(pages.valid(rig, variant));
    assert_eq!(pages.variant_location(rig, 5), None);
    assert_eq!(pages.variant_location(EntityRigId(0), 9), None);
}

// Animated face and body skin layers draw from equipment pages on skin rig ids; admitting
// catalog textures for skin models must keep accepting those.
#[test]
fn skin_models_keep_drawing_their_animated_layer_images() {
    let raster = EquipmentRaster {
        width: 2,
        height: 2,
        rgba8: vec![9; 16].into(),
    };
    let (pages, locations) = ActorArtworkPages::default().with_equipment_rasters(&[raster]);
    assert!(pages.valid(crate::skin_rig_id(0), locations[0].unwrap()));
}

#[test]
fn equipment_rasters_group_by_size_and_validate_by_location() {
    let raster = |side: u16| EquipmentRaster {
        width: side,
        height: side,
        rgba8: vec![9; usize::from(side) * usize::from(side) * 4].into(),
    };
    let bad = EquipmentRaster {
        width: 2,
        height: 2,
        rgba8: vec![0; 3].into(),
    };
    let (pages, locations) = ActorArtworkPages::default().with_equipment_rasters(&[
        raster(2),
        bad,
        raster(4),
        raster(2),
    ]);
    assert_eq!(pages.pages().len(), 2);
    assert_ne!(pages.identity(), [0; 32]);
    assert_eq!(locations[1], None);
    let (first, second) = (locations[0].unwrap(), locations[3].unwrap());
    assert_eq!((first.page(), second.page()), (1, 1));
    assert_eq!((first.layer(), second.layer()), (0, 1));
    assert_eq!(locations[2].unwrap().page(), 2);
    let equipment_rig = crate::equipment_rig_id(3);
    assert!(pages.valid(equipment_rig, first));
    let unknown = ActorArtworkLocation { layer: 9, ..first };
    assert!(!pages.valid(equipment_rig, unknown));
}
#[test]
fn review_render_artwork_identity_distinguishes_page_boundaries() {
    let marker = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([2, 0, 1, 0]),
    };
    let pair = EquipmentRaster {
        width: 2,
        height: 1,
        rgba8: Arc::from([9; 8]),
    };
    let pixel = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([9; 4]),
    };
    let (a, _) = ActorArtworkPages::default().with_equipment_rasters(&[marker.clone(), pair]);
    let (b, _) = ActorArtworkPages::default().with_equipment_rasters(&[
        marker.clone(),
        marker,
        pixel.clone(),
        pixel,
    ]);
    assert_ne!(a.identity(), b.identity());
}
