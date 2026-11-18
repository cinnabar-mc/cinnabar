use super::*;

#[test]
fn fence_thumbnails_keep_the_mesh_surface_order() {
    const TILE_SIDE: u8 = 16;
    let texture = assets::TextureArray {
        layers: 1,
        mips: vec![assets::TextureMip {
            size: u32::from(TILE_SIDE),
            rgba8: (0..TILE_SIDE)
                .flat_map(|y| {
                    (0..TILE_SIDE).flat_map(move |x| {
                        [
                            x * 16,
                            y * 16,
                            (x ^ y) * 16,
                            if (x + y).is_multiple_of(5) { 0 } else { 255 },
                        ]
                    })
                })
                .collect::<Vec<_>>()
                .into(),
        }]
        .into(),
    };
    let overlay = BlockOverlay {
        textures: vec![texture],
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
        let mut model = Model::build(
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
        let mut expected = vec![0; actual.rgba8.len()];
        // Isolated faces have no depth conflicts; overlay them in the mesh's supplied order.
        for face in std::mem::take(&mut model.faces) {
            model.faces = vec![face];
            let layer = model.raster();
            for (output, source) in expected
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(layer.rgba8.as_chunks::<4>().0)
            {
                if source[3] != 0 {
                    *output = *source;
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
        assert_eq!(
            actual.rgba8.as_ref(),
            expected.as_slice(),
            "{flags}, {render_kind:?}"
        );
    }
}
