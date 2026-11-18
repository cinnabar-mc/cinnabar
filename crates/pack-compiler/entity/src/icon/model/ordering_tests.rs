use super::*;

/// Rasters shared GUI faces with ordinary depth ordering.
fn raster_quads(
    quads: impl IntoIterator<Item = assets::gui_item::GuiBlockQuad>,
    texture: &IconSprite,
) -> IconSprite {
    Model::textured(
        quads.into_iter().map(|quad| TexturedQuad {
            corners: quad.corners,
            uvs: quad.uvs,
            texture,
            face_dimming: true,
        }),
        assets::gui_item::project_cube,
        1.0,
    )
    .raster()
}

#[test]
fn fence_thumbnails_match_gui_surface_order() {
    for cutout in [false, true] {
        let side = assets::TILE_SIZE as usize;
        let texture = IconSprite {
            width: side as u16,
            height: side as u16,
            rgba8: (0..side * side)
                .flat_map(|pixel| {
                    [
                        (pixel % side * 16) as u8,
                        (pixel / side * 16) as u8,
                        120,
                        if cutout && pixel % 3 == 0 { 0 } else { 255 },
                    ]
                })
                .collect(),
        };
        let overlay = BlockOverlay {
            textures: vec![assets::TextureArray {
                layers: 1,
                mips: vec![assets::TextureMip {
                    size: side as u32,
                    rgba8: texture.rgba8.to_vec().into(),
                }]
                .into(),
            }],
            ..Default::default()
        };
        let materials = [
            Material::unvaried(),
            Material {
                texture: assets::TextureRef::new(1, 0).unwrap(),
                flags: assets::MATERIAL_FLAG_ALPHA_CUTOUT,
                ..Material::unvaried()
            },
        ];
        for (flags, render_kind) in [
            (assets::MODEL_TEMPLATE_FLAG_FENCE_WOOD, Default::default()),
            (assets::MODEL_TEMPLATE_FLAG_FENCE_NETHER, Default::default()),
            (
                assets::MODEL_TEMPLATE_FLAG_FENCE_WOOD,
                assets::facts::BlockRenderKind::BambooFence,
            ),
        ] {
            let templates = [ModelTemplate {
                quad_start: 0,
                quad_count: 0,
                flags,
            }];
            let model = Model::build(
                Parts {
                    materials: &materials,
                    templates: &templates,
                    quads: &[],
                    textures: Textures::Overlay(&overlay, None),
                    tint: None,
                },
                VisualKind::Model,
                [1; 6],
                Some(0),
                render_kind,
            )
            .unwrap();
            let actual = model.raster();
            let quads = block_gui_quads(
                render_kind,
                VisualKind::Model,
                [1; 6],
                Some(0),
                &templates,
                &[],
            )
            .unwrap();
            let mut expected = vec![0; SIDE * SIDE * 4];
            for quad in quads.iter().copied().filter(|quad| quad.visible_in_gui()) {
                let face = raster_quads([quad], &texture);
                for (target, source) in expected
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(face.rgba8.as_chunks::<4>().0)
                {
                    if source[3] != 0 {
                        *target = *source;
                    }
                }
            }
            assert!(
                expected
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] != 0)
            );
            if render_kind != assets::facts::BlockRenderKind::BambooFence {
                assert_ne!(
                    raster_quads(quads, &texture).rgba8.as_ref(),
                    expected,
                    "the fixture must distinguish paint order from depth order"
                );
            }
            assert_eq!(actual.rgba8.as_ref(), expected);
        }
    }
}
