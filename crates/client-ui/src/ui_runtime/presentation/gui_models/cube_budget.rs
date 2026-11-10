//! GUI-only cube sources remain optional under the real GUI atlas budget.

use super::*;
use assets::{
    IconBlockSheet, IconEntry, IconSprite, ItemVisualDefinitionRoute, ItemVisualKey,
    RuntimeIconCatalog,
};

/// Fills a bounded shelf with distinct opaque sheets and keeps the carrier's sheet-count limit.
/// Required equipment textures fill any remaining atlas pages in larger budget fixtures.
fn sheets_at_capacity(pages: usize) -> Vec<IconSprite> {
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    let mut cutout = vec![255; usize::from(width) * usize::from(height) * 4];
    cutout[3] = 0;
    let mut sheets = vec![IconSprite {
        width,
        height,
        rgba8: cutout.into(),
    }];
    let mut atlas = atlas::Atlas::new(1, pages);
    let limit = assets::MAX_ICON_BLOCK_SHEETS.min(assets::MAX_ICON_SPRITES);
    for index in 1..limit {
        let [red, green, blue, _] = (index as u32).to_le_bytes();
        let rgba8 = [red, green, blue, 255].repeat(usize::from(width) * usize::from(height));
        if atlas.insert([width, height], &rgba8).is_err() {
            assert_eq!(atlas.page_count(), pages);
            return sheets;
        }
        sheets.push(IconSprite {
            width,
            height,
            rgba8: rgba8.into(),
        });
    }
    panic!("the requested shelf capacity exceeds the carrier's sheet limit; use equipment pages")
}

/// Builds original compiled item routes and admitted sheets without needing game carriers.
fn catalog(sheets: Vec<IconSprite>) -> (RuntimeEntityAssets, RuntimeIconCatalog) {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","geometry":{"default":"geometry.budget"},"render_controllers":["controller.render.budget"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.budget","texture_width":64,"texture_height":64},"bones":[{"name":"body","cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]},{"name":"rightItem","parent":"body","pivot":[-6,12,0]},{"name":"leftItem","parent":"body","pivot":[6,12,0]}]}]}"#;
    let render = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.budget":{"geometry":"Geometry.default"}}}"#;
    let mut compiled = pack_entity_compiler::compile_entity_pack(vec![
        ("entity/budget.json".into(), entity.to_vec()),
        ("models/entity/budget.geo.json".into(), geometry.to_vec()),
        ("render_controllers/budget.json".into(), render.to_vec()),
    ])
    .unwrap()
    .unwrap()
    .assets;
    compiled.block_visual_count = sheets.len() as u32 + 1;
    compiled.item_visuals = (0..sheets.len())
        .map(|index| assets::ItemVisualDefinition {
            grip: assets::item::grip::ItemGrip::Unresolved,
            glint: assets::item::glint::ItemGlint::Unresolved,
            fuel: assets::item::fuel::ItemFuel::Unresolved,
            durability: assets::item::durability::ItemDurability::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            key: ItemVisualKey {
                identifier: format!("fixture:cube_{index:05}").into(),
                metadata: 0,
            },
            source: 0,
            route: ItemVisualDefinitionRoute::BlockItem {
                block_visual: assets::BlockVisualId(index as u32 + 1),
            },
            first_person: assets::ItemDisplayTransform::identity(),
            third_person: assets::ItemDisplayTransform::identity(),
            dropped: assets::ItemDisplayTransform::identity(),
        })
        .collect();
    let entities = RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap();
    let entries: Vec<_> = entities
        .item_visuals()
        .iter()
        .enumerate()
        .map(|(index, definition)| IconEntry {
            identifier: definition.key.identifier.clone(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect();
    let blocks: Vec<_> = (0..sheets.len())
        .map(|index| IconBlockSheet {
            visual: assets::BlockVisualId(index as u32 + 1),
            sprite: index as u32,
        })
        .collect();
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            entities.source_manifest_sha256(),
            &sheets,
            &entries,
            &blocks,
        )
        .unwrap(),
    )
    .unwrap();
    (entities, icons)
}

/// Adds distinct GUI-only artwork and a shared held sheet after one full required page.
fn opaque_gui_catalog() -> (RuntimeEntityAssets, RuntimeIconCatalog) {
    let (entities, icons) = catalog(sheets_at_capacity(1));
    let mut sprites = icons.sprites().to_vec();
    let mut gui = sprites[0].clone();
    gui.rgba8 = [31, 47, 59, 255]
        .repeat(usize::from(gui.width) * usize::from(gui.height))
        .into();
    let gui_sheet = IconBlockSheet {
        visual: icons.block_sheets()[0].visual,
        sprite: sprites.len() as u32,
    };
    sprites.push(gui);
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_gui_cube_sheets(
            entities.source_manifest_sha256(),
            &sprites,
            icons.entries(),
            &icons.block_sheets()[1..],
            icons.block_models(),
            &[],
            &[gui_sheet, icons.block_sheets()[1]],
        )
        .unwrap(),
    )
    .unwrap();
    (entities, icons)
}

/// Fills the requested number of pages with distinct required equipment textures.
fn equipment(entities: &RuntimeEntityAssets, pages: usize) -> Arc<assets::RuntimeEquipmentCatalog> {
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            entities.source_manifest_sha256(),
            Vec::new(),
            (0..pages)
                .map(|page| assets::EquipmentTexture {
                    identifier: format!("textures/fixture/budget_{page:02}").into(),
                    width: side,
                    height: side,
                    rgba8: [page as u8, 17, 23, 255]
                        .repeat(usize::from(side) * usize::from(side))
                        .into(),
                })
                .collect(),
        )
        .unwrap(),
    )
}

/// Keeps the same thumbnail control before and after optional model installation or reclamation.
fn node(icon: IconRef) -> UiNode {
    let bounds = ui::UiRect::new(
        ui::UiPoint::new(0.0, 0.0).unwrap(),
        ui::UiPoint::new(32.0, 32.0).unwrap(),
    )
    .unwrap();
    UiNode::new(ui::UiNodeId::new(1), None, bounds).with_visual(UiVisual::Sprite {
        texture_page: icon.page,
        uv: icon.uv,
        color: [255; 4],
    })
}

#[test]
fn full_required_cube_atlas_keeps_cutout_thumbnail_without_failing_installation() {
    let cube_pages = 1;
    let (entities, icons) = catalog(sheets_at_capacity(cube_pages));
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    presentation.set_equipment_catalog(Some(equipment(&entities, MODEL_PAGES - cube_pages)));
    let cutout = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    let opaque = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    assert_eq!(presentation.gui_models.sources.required_pages, MODEL_PAGES);
    assert_eq!(presentation.gui_models.sources.pages.len(), MODEL_PAGES);
    assert!(presentation.gui_models.sources.optional_models.is_empty());
    let mut nodes = [node(cutout), node(opaque)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(
        nodes[0], thumbnail,
        "budget pressure retains the cutout thumbnail"
    );
    assert!(
        matches!(nodes[1].visual(), UiVisual::Mesh(_)),
        "opaque cubes remain geometry"
    );
}

#[test]
fn full_required_atlas_keeps_opaque_gui_override_on_its_thumbnail() {
    let (entities, icons) = opaque_gui_catalog();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let equipment = equipment(&entities, MODEL_PAGES - 1);
    presentation.set_equipment_catalog(Some(Arc::clone(&equipment)));
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .expect("GUI-only artwork must not exhaust required equipment pages");
    assert_eq!(presentation.gui_models.sources.required_pages, MODEL_PAGES);
    assert_eq!(presentation.gui_models.sources.pages.len(), MODEL_PAGES);
    for texture in equipment.textures() {
        assert!(
            presentation
                .gui_models
                .sources
                .textures
                .contains_key(&atlas::key([texture.width, texture.height], &texture.rgba8))
        );
    }
    let gui = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    let shared = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    let mut nodes = [node(gui), node(shared)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(
        nodes[0], thumbnail,
        "unadmitted GUI artwork keeps its thumbnail"
    );
    assert!(matches!(nodes[1].visual(), UiVisual::Mesh(_)));
    let held = &presentation.gui_models.sources.held["fixture:cube_00001"][&0];
    assert!(
        !held.is_sprite(),
        "shared held artwork keeps its block model"
    );
}

#[test]
fn opaque_gui_only_pages_can_be_reclaimed_without_dropping_shared_held_artwork() {
    let (entities, icons) = opaque_gui_catalog();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    assert_eq!(presentation.gui_models.sources.required_pages, 1);
    assert_eq!(presentation.gui_models.sources.pages.len(), 2);
    let gui = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    let shared = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    let mut nodes = [node(gui), node(shared)];
    presentation.apply_gui_models(&mut nodes);
    assert!(
        nodes
            .iter()
            .all(|node| matches!(node.visual(), UiVisual::Mesh(_)))
    );
    let mut faded = [node(gui).with_visual(UiVisual::Sprite {
        texture_page: gui.page,
        uv: gui.uv,
        color: [255, 255, 255, 128],
    })];
    presentation.apply_gui_models(&mut faded);
    assert!(matches!(faded[0].visual(), UiVisual::Mesh(_)));
    let icons = presentation.icon_catalog.as_deref().unwrap();
    let sheet = &icons.sprites()[icons.block_sheets()[0].sprite as usize];
    let shared_key = atlas::key([sheet.width, sheet.height], &sheet.rgba8);
    let source = presentation.gui_models.sources.textures[&shared_key];
    let required = presentation.gui_models.sources.pages[0].pixels().to_vec();
    let first = presentation.textures.dynamic_start() + MODEL_PAGE;
    assert!(presentation.gui_models.discard_optional_models(first));
    assert_eq!(presentation.gui_models.sources.pages.len(), 1);
    assert_eq!(presentation.gui_models.sources.pages[0].pixels(), required);
    assert_eq!(
        presentation.gui_models.sources.textures[&shared_key],
        source
    );
    let held = &presentation.gui_models.sources.held["fixture:cube_00001"][&0];
    assert!(!held.is_sprite());
    assert_eq!(
        held.model(presentation.icon_catalog.as_deref().unwrap())
            .unwrap()
            .source,
        shared
    );
    let mut nodes = [node(gui), node(shared)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(nodes[0], thumbnail);
    assert!(matches!(nodes[1].visual(), UiVisual::Mesh(_)));
}

#[test]
fn gui_cube_pages_follow_gui_coverage_independently_of_held_sheets() {
    let (entities, icons) = catalog(sheets_at_capacity(1));
    let [cutout, opaque] = [icons.block_sheets()[0], icons.block_sheets()[1]];
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_gui_cube_sheets(
            entities.source_manifest_sha256(),
            icons.sprites(),
            icons.entries(),
            icons.block_sheets(),
            icons.block_models(),
            &[],
            &[
                IconBlockSheet {
                    sprite: opaque.sprite,
                    ..cutout
                },
                IconBlockSheet {
                    sprite: cutout.sprite,
                    ..opaque
                },
            ],
        )
        .unwrap(),
    )
    .unwrap();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let cutout = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    let opaque = presentation.item_icon("fixture:cube_00000", 0).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    assert_eq!(presentation.gui_models.sources.required_pages, 1);
    assert_eq!(presentation.gui_models.sources.pages.len(), 2);
    let required = presentation.gui_models.sources.pages[0].pixels().to_vec();
    let mut drawn = [node(cutout), node(opaque)];
    presentation.apply_gui_models(&mut drawn);
    assert!(
        drawn
            .iter()
            .all(|node| matches!(node.visual(), UiVisual::Mesh(_)))
    );
    let first = presentation.textures.dynamic_start() + MODEL_PAGE;
    assert!(presentation.gui_models.discard_optional_models(first));
    assert_eq!(presentation.gui_models.sources.pages.len(), 1);
    assert_eq!(presentation.gui_models.sources.pages[0].pixels(), required);
    assert!(
        !presentation
            .gui_models
            .sources
            .textures
            .contains_key(&atlas::key(
                assets::BLOCK_ITEM_SHEET_SIZE,
                &presentation.icon_catalog.as_deref().unwrap().sprites()[0].rgba8,
            )),
        "reclaimed optional-page textures are no longer addressable"
    );
    let mut nodes = [node(cutout), node(opaque)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(nodes[0], thumbnail);
    assert!(matches!(nodes[1].visual(), UiVisual::Mesh(_)));
}

#[test]
fn oversized_pack_keeps_fallback_artwork_instead_of_failing_gui_startup() {
    // Pack equipment alone overflows every model page, as larger-resolution packs can.
    let (entities, icons) = catalog(sheets_at_capacity(1));
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    let side = render_model::UI_MODEL_ATLAS_SIDE as u16;
    presentation.set_equipment_catalog(Some(Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            entities.source_manifest_sha256(),
            Vec::new(),
            (0..=MODEL_PAGES)
                .map(|page| assets::EquipmentTexture {
                    identifier: format!("textures/fixture/oversized_{page:02}").into(),
                    width: side,
                    height: side,
                    rgba8: [page as u8, 31, 47, 255]
                        .repeat(usize::from(side) * usize::from(side))
                        .into(),
                })
                .collect(),
        )
        .unwrap(),
    )));
    let cube = presentation.item_icon("fixture:cube_00001", 0).unwrap();
    presentation
        .set_gui_models(&RuntimeAssets::diagnostic(), &entities)
        .expect("a full model atlas must not fail GUI startup");
    let fallbacks = presentation.gui_models.sources.fallbacks;
    assert_eq!(
        fallbacks.equipment, 1,
        "equipment fills the atlas before cubes"
    );
    assert!(fallbacks.cubes > 0);
    assert_eq!(presentation.gui_models.sources.pages.len(), MODEL_PAGES);
    let mut nodes = [node(cube)];
    let thumbnail = nodes[0].clone();
    presentation.apply_gui_models(&mut nodes);
    assert_eq!(
        nodes[0], thumbnail,
        "cubes past the atlas keep their thumbnail"
    );
    let flame = assets::ParticleTexture {
        path: assets::ACTOR_FLAME_TEXTURE.into(),
        width: 16,
        height: 32,
        rgba8: vec![255; 16 * 32 * 4].into(),
    };
    presentation.set_gui_fire_texture(Some(&flame));
    assert!(presentation.gui_models.fire.frames.is_empty());
    assert_eq!(presentation.gui_models.sources.pages.len(), MODEL_PAGES);
}
