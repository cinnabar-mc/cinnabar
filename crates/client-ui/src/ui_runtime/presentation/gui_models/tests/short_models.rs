//! Original shortened cube fixtures exercise both actual GUI admission paths.

use super::*;

/// Encodes an exact six-face model with original fractional-alpha textures and shared provenance.
pub(super) fn world(manifest: [u8; 32], height: u16, alpha: u8) -> RuntimeAssets {
    world_with_width(manifest, height, alpha, 256)
}

/// Encodes an original centered model with the requested width in fixed-point block units.
pub(super) fn world_with_width(
    manifest: [u8; 32],
    height: u16,
    alpha: u8,
    width: u16,
) -> RuntimeAssets {
    world_with_material(
        manifest,
        height,
        alpha,
        width,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    )
}

/// Encodes the same original model with a selected material alpha policy.
pub(super) fn world_with_material(
    manifest: [u8; 32],
    height: u16,
    alpha: u8,
    width: u16,
    flags: u32,
) -> RuntimeAssets {
    world_with_model_flags(manifest, height, alpha, width, flags, 0)
}

/// Encodes a centered model, including all connection masks for pane templates.
pub(super) fn world_with_model_flags(
    manifest: [u8; 32],
    height: u16,
    alpha: u8,
    width: u16,
    flags: u32,
    model_flags: u32,
) -> RuntimeAssets {
    use assets::*;
    let mut visual = BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
    if height == 256 && width == 256 {
        visual.flags = BlockFlags::OCCLUDES_FULL_FACE;
    }
    visual.kind = VisualKind::Model;
    visual.support = VisualSupport::Exact;
    visual.faces = [1; 6];
    visual.model_template = 0;
    let quads: Vec<_> = BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (corners, uvs) = gui_item::cube_face(face);
            ModelQuad {
                positions: corners.map(|[x, y, z]| {
                    [
                        (x * f32::from(width) + (256.0 - f32::from(width)) * 0.5) as i16,
                        (y * f32::from(height)) as i16,
                        (z * 256.0) as i16,
                    ]
                }),
                uvs: uvs.map(|uv| uv.map(|value| (value * 4096.0) as u16)),
                material: 1,
                flags: face.model_quad_face_id(),
            }
        })
        .collect();
    let template_count = if model_flags & MODEL_TEMPLATE_FLAG_PANE != 0 {
        16
    } else {
        1
    };
    let compiled = CompiledAssets {
        visuals: vec![
            BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary),
            visual,
        ]
        .into(),
        light_properties: vec![LightProperties::OPAQUE_DARK; 2].into(),
        hashed: Box::new([]),
        materials: vec![
            Material::unvaried(),
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags,
                ..Material::unvaried()
            },
        ]
        .into(),
        model_templates: (0..template_count)
            .map(|index| ModelTemplate {
                quad_start: (index * quads.len()) as u32,
                quad_count: quads.len() as u32,
                flags: model_flags,
            })
            .collect(),
        model_quads: quads.repeat(template_count).into(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 2,
            mips: (0..MIP_COUNT)
                .map(|level| {
                    let size = TILE_SIZE >> level;
                    TextureMip {
                        size,
                        rgba8: [255, 0, 255, 255]
                            .repeat((size * size) as usize)
                            .into_iter()
                            .chain([31, 47, 59, alpha].repeat((size * size) as usize))
                            .collect::<Vec<_>>()
                            .into(),
                    }
                })
                .collect::<Vec<_>>()
                .into(),
        })]
        .into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: manifest,
            block_registry_sha256: [1; 32],
            light_registry_sha256: [2; 32],
            biome_registry_sha256: [3; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

#[test]
fn pane_fixture_round_trips_every_connection_mask() {
    let world = world_with_model_flags(
        [0; 32],
        256,
        128,
        32,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MODEL_TEMPLATE_FLAG_PANE,
    );
    let block = world.resolve(assets::NetworkIdMode::Sequential, 1);
    let base = block.model_template().unwrap() as usize;
    for mask in 0..16 {
        let template = world.model_templates()[base + mask];
        let start = template.quad_start as usize;
        let quads = &world.model_quads()[start..start + template.quad_count as usize];
        assert_eq!(quads.len(), assets::BlockFace::ALL.len());
        assert!(quads.iter().all(|quad| quad.material == 1));
    }
}

/// Replaces the carried face source while keeping real item and shaped-thumbnail bindings.
fn icons(
    entities: &RuntimeEntityAssets,
    original: &assets::RuntimeIconCatalog,
    alpha: u8,
) -> assets::RuntimeIconCatalog {
    let mut sprites = original.sprites().to_vec();
    let last = sprites.last_mut().unwrap();
    last.rgba8 = [31, 47, 59, alpha]
        .repeat(usize::from(last.width) * usize::from(last.height))
        .into();
    let models: Vec<_> = original
        .entries()
        .iter()
        .map(|entry| assets::IconBlockModel {
            held_sprite: None,
            sprite: entry.sprite,
            visual: assets::BlockVisualId(1),
        })
        .collect();
    assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_blocks(
            entities.source_manifest_sha256(),
            &sprites,
            original.entries(),
            original.block_sheets(),
            &models,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn normalized_opaque_and_cutout_sheets_keep_the_shortened_gui_model() {
    for alpha in [
        assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD - 1,
        assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD,
        192,
        255,
    ] {
        let (base, entities) = cubes::catalog_for(assets::END_PORTAL_FRAME_IDENTIFIER, 255, 255);
        let icons = icons(&entities, base.icon_catalog.as_deref().unwrap(), alpha);
        let mut presentation = UiPresentationRuntime::with_hud_and_icons(
            ui_engine::test_support::fixture_font(),
            crate::test_support::fixture_hud(),
            Arc::new(icons),
        )
        .unwrap();
        let source_world = world(entities.source_manifest_sha256(), 13 * 16, alpha);
        presentation
            .set_gui_models(&source_world, &entities)
            .unwrap();
        let icon = presentation
            .item_icon(assets::END_PORTAL_FRAME_IDENTIFIER, 0)
            .unwrap();
        let actual = &presentation.gui_models.sources.models[&icon_key(icon)];
        let first = (presentation.textures.dynamic_start() + MODEL_PAGE) as u16;
        let expected = block_models::mesh(
            &source_world,
            assets::BlockVisualId(1),
            &mut atlas::Atlas::new(first, MODEL_PAGES),
            Default::default(),
        )
        .unwrap();
        assert_eq!(actual.indices(), expected.indices());
        assert_eq!(
            actual
                .vertices()
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>(),
            expected
                .vertices()
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>()
        );
        let full = world(entities.source_manifest_sha256(), 256, alpha);
        let full = block_models::mesh(
            &full,
            assets::BlockVisualId(1),
            &mut atlas::Atlas::new(first, MODEL_PAGES),
            Default::default(),
        )
        .unwrap();
        assert_ne!(
            actual
                .vertices()
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>(),
            full.vertices()
                .iter()
                .map(|vertex| vertex.position)
                .collect::<Vec<_>>(),
            "shortened faces must never draw as a full cube"
        );
        assert!(
            presentation
                .gui_models
                .sources
                .pages
                .iter()
                .flat_map(|page| page.pixels().as_chunks::<4>().0)
                .all(|texel| matches!(texel[3], 0 | 255))
        );
    }
}
