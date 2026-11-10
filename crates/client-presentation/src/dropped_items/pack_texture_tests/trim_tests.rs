//! Trimmed armor keeps its trim on the ground and in item frames, as its inventory icon does.
use super::*;

const CHESTPLATE: &str = "minecraft:diamond_chestplate";
const ICON: [u8; 4] = [20, 30, 40, 255];
const GOLD: [u8; 4] = [240, 170, 20, 255];

/// A dropped diamond chestplate with original one-pixel artwork, trim mask and palettes.
fn trim_fixture() -> (UiPresentationRuntime, WorldStream) {
    let (mut icons, stream) = fixture_item(CHESTPLATE, 0, 0);
    let texture = |identifier: &str, pixel: [u8; 4]| assets::EquipmentTexture {
        identifier: identifier.into(),
        width: 1,
        height: 1,
        rgba8: Arc::from(pixel),
    };
    let key = [100, 100, 100, 255];
    icons.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [6; 32],
            Vec::new(),
            vec![
                texture("textures/trims/items/chestplate_trim", key),
                texture("textures/trims/color_palettes/trim_palette", key),
                texture("textures/trims/color_palettes/gold", GOLD),
            ],
        )
        .unwrap(),
    )));
    (icons, stream)
}

fn gold_sentry() -> render_api::ItemTrim {
    render_api::ItemTrim {
        material: "gold".into(),
        pattern: "sentry".into(),
    }
}

/// The first texel of a resolved sprite model.
fn sprite_texel(cache: &ModelCache, model: u32) -> [u8; 4] {
    match &cache.models[model as usize] {
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            sprite.rgba8[..4].try_into().unwrap()
        }
        _ => panic!("armor draws as an extruded sprite"),
    }
}

#[test]
fn dropped_trimmed_armor_draws_its_trim() {
    let (icons, stream) = trim_fixture();
    let mut views = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut views);
    for (trim, expected) in [(None, ICON), (Some(gold_sentry()), GOLD)] {
        views[0].item.trim = trim;
        let mut cache = ModelCache::default();
        cache.begin_frame();
        cache.finish_demand();
        let preparation = DropPreparation {
            view: &views[0],
            native_origin: views[0].position,
            camera: None,
            frustum: None,
        };
        let prepared =
            DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, None, &preparation)
                .unwrap();
        let mut instances = Vec::new();
        prepared.append(&views[0], &mut instances, |_| (15, 15));
        assert_eq!(sprite_texel(&cache, instances[0].model), expected);
    }
}

#[test]
fn framed_trimmed_armor_draws_its_trim() {
    let (icons, _) = trim_fixture();
    for (trim, expected) in [(None, ICON), (Some(gold_sentry()), GOLD)] {
        let placement = render_model::StaticItemPlacement {
            identifier: CHESTPLATE.into(),
            metadata: 0,
            shield_pattern: None,
            trim,
            potted_block: None,
            carried_block: None,
            allow_block_models: true,
            world_from_item: render_model::item_frame_item_transform([0; 3], 2, 0.0),
            world_from_block: None,
            light: None,
        };
        let mut cache = ModelCache::default();
        cache.begin_frame();
        cache.finish_demand();
        let model = DroppedItemPublisher::placement_model(
            &mut cache,
            &icons,
            None,
            &placement,
            icons.startup_item_icon(CHESTPLATE, 0),
        )
        .unwrap();
        assert_eq!(sprite_texel(&cache, model), expected);
    }
}

#[test]
fn trimmed_world_sprites_release_departed_pack_catalogs() {
    let (mut icons, _) = trim_fixture();
    let pack = Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [7; 32],
            Vec::new(),
            vec![assets::EquipmentTexture {
                identifier: "textures/trims/color_palettes/gold".into(),
                width: 1,
                height: 1,
                rgba8: Arc::from([255, 0, 0, 255]),
            }],
        )
        .unwrap(),
    );
    let weak = Arc::downgrade(&pack);
    icons.set_preview_pack_equipment(Some(pack));
    let icon = icons.startup_item_icon(CHESTPLATE, 0).unwrap();
    let artwork = icons
        .world_trim_artwork(&CHESTPLATE.into(), icon, Some(&gold_sentry()), true)
        .unwrap();
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    let model = cache
        .model(
            ModelKey::Trim(artwork.clone()),
            || artwork.cost(&icons, 16),
            || artwork.model(&icons, 16),
        )
        .unwrap();
    assert_eq!(
        sprite_texel(&cache, model),
        [255, 0, 0, 255],
        "the active pack's palette replaces the carrier's"
    );
    icons.set_preview_pack_equipment(None);
    drop(artwork);
    cache.finish_frame();
    cache.begin_frame();
    cache.finish_frame();
    assert!(
        weak.upgrade().is_none(),
        "an absent trimmed consumer must release its pack catalog"
    );
}

#[test]
fn trims_sharing_a_material_share_one_world_sprite() {
    let (icons, _) = trim_fixture();
    let icon = icons.startup_item_icon(CHESTPLATE, 0).unwrap();
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    let models = ["sentry", "dune"].map(|pattern| {
        let trim = render_api::ItemTrim {
            pattern: pattern.into(),
            ..gold_sentry()
        };
        let artwork = icons
            .world_trim_artwork(&CHESTPLATE.into(), icon, Some(&trim), true)
            .unwrap();
        cache
            .model(
                ModelKey::Trim(artwork.clone()),
                || artwork.cost(&icons, 16),
                || artwork.model(&icons, 16),
            )
            .unwrap()
    });
    assert_eq!(
        models[0], models[1],
        "an icon's trim mask follows the slot, so patterns draw identical pixels"
    );
}

#[test]
fn trimmed_world_sprite_cost_measures_the_built_sprite() {
    // A half-covered 2x2 icon under a 4x4 mask with clear, faint and solid texels.
    let base: Vec<u8> = [
        [20, 30, 40, 255],
        [0; 4],
        [20, 30, 40, 1],
        [20, 30, 40, 255],
    ]
    .concat();
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            [5; 32],
            &[assets::IconSprite {
                width: 2,
                height: 2,
                rgba8: Arc::from(base),
            }],
            &[assets::IconEntry {
                identifier: CHESTPLATE.into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut icons, _) = fixture_item_with_catalog(CHESTPLATE, 0, 0, catalog);
    let key = [100, 100, 100];
    let mask: Vec<u8> = (0..16u8)
        .flat_map(|texel| {
            let alpha = [0, 1, 2, 255][usize::from(texel % 4)];
            [key[0], key[1], key[2], alpha]
        })
        .collect();
    icons.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [6; 32],
            Vec::new(),
            vec![
                assets::EquipmentTexture {
                    identifier: "textures/trims/items/chestplate_trim".into(),
                    width: 4,
                    height: 4,
                    rgba8: Arc::from(mask),
                },
                assets::EquipmentTexture {
                    identifier: "textures/trims/color_palettes/trim_palette".into(),
                    width: 1,
                    height: 1,
                    rgba8: Arc::from([100, 100, 100, 255]),
                },
                assets::EquipmentTexture {
                    identifier: "textures/trims/color_palettes/gold".into(),
                    width: 1,
                    height: 1,
                    rgba8: Arc::from(GOLD),
                },
            ],
        )
        .unwrap(),
    )));
    let icon = icons.startup_item_icon(CHESTPLATE, 0).unwrap();
    for native in [true, false] {
        let artwork = icons
            .world_trim_artwork(&CHESTPLATE.into(), icon, Some(&gold_sentry()), native)
            .unwrap();
        for max_side in [16, 2, 1] {
            let sprite = artwork.sprite(&icons, max_side).unwrap();
            let threshold = if native { 2 } else { 1 };
            let vertices = render_model::sampled_sprite_vertex_count(
                sprite.width as usize,
                sprite.height as usize,
                |x, y| sprite.rgba8[(y * sprite.width as usize + x) * 4 + 3] >= threshold,
            );
            let built = vertices.and_then(|vertices| {
                render_model::dropped_item::ItemModelCost::new(
                    sprite.width.max(sprite.height),
                    1,
                    sprite.rgba8.len(),
                    vertices,
                )
            });
            assert_eq!(
                artwork.cost(&icons, max_side),
                built,
                "native {native}, side {max_side}"
            );
        }
    }
}
