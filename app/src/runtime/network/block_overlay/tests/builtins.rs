use super::*;

#[test]
fn versioned_isotropic_bottom_keeps_positional_turns_available() {
    let mut material = CustomMaterialInstance::with_authored_defaults("*", "lucky");
    material.packed_bools |= 2;
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:isotropic_cube",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block_v1".into()),
                    materials: Some(vec![material].into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).unwrap();
    let visual = compiled.overlay.visuals[0];
    assert_eq!(visual.kind, VisualKind::Cube);
    for id in visual.faces {
        let flags = compiled.overlay.materials[id as usize].flags;
        assert_ne!(flags & assets::MATERIAL_FLAG_ISOTROPIC, 0);
        assert_eq!(
            flags & assets::MATERIAL_FLAG_UV_ROTATION_MASK,
            0,
            "every face leaves the positional rotation code available"
        );
    }
    let down = compiled.overlay.materials[visual.faces[BlockFace::Down as usize] as usize];
    for corner in 0..4 {
        assert_eq!(
            render_scene::greedy_texture_uv(meshing::Face::NegativeY, corner, 1, 1, down.flags),
            render_scene::greedy_texture_uv(
                meshing::Face::NegativeY,
                corner,
                1,
                1,
                assets::MATERIAL_FLAG_UV_ROTATE_180,
            ),
            "the bottom keeps its authored half-turn"
        );
    }
}

#[test]
fn versioned_builtin_full_block_resolves_textured_cube_without_pack_geometry() {
    let view = view();
    let blocks = CustomBlocks {
        blocks: [
            "minecraft:geometry.full_block",
            "minecraft:geometry.full_block_v1",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, identifier)| {
            block(
                &format!("test:full_block_{index}"),
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some(identifier.into()),
                        materials: Some(Box::new([
                            CustomMaterialInstance::with_authored_defaults("side", "gen"),
                            CustomMaterialInstance::with_authored_defaults("up", "lucky"),
                            CustomMaterialInstance::with_authored_defaults("down", "lucky"),
                        ])),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        })
        .collect(),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).unwrap();
        assert_eq!(compiled.gaps.missing_geometry, 0);
        assert_eq!(compiled.gaps.missing_textures, 0);
        for visual in &compiled.overlay.visuals {
            assert_eq!(visual.kind, VisualKind::Cube);
            assert!(
                visual
                    .flags
                    .contains(assets::BlockFlags::OCCLUDES_FULL_FACE)
            );
            assert_eq!(visual.support, VisualSupport::Exact);
            assert!(visual.faces.iter().all(|material| *material != 0));
        }
        let [modern, original]: [assets::BlockVisual; 2] =
            compiled.overlay.visuals[..].try_into().unwrap();
        for face in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::Up,
            BlockFace::North,
            BlockFace::South,
        ] {
            assert_eq!(modern.faces[face as usize], original.faces[face as usize]);
        }
        let down = |visual: assets::BlockVisual| {
            compiled.overlay.materials[visual.faces[BlockFace::Down as usize] as usize]
        };
        assert_eq!(down(modern).texture, down(original).texture);
        assert_eq!(down(modern).animation, down(original).animation);
        for (corner, expected) in [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
            .into_iter()
            .enumerate()
        {
            let original_uv = render_scene::greedy_texture_uv(
                meshing::Face::NegativeY,
                corner as u32,
                1,
                1,
                down(original).flags,
            );
            let modern_uv = render_scene::greedy_texture_uv(
                meshing::Face::NegativeY,
                corner as u32,
                1,
                1,
                down(modern).flags,
            );
            assert_eq!(original_uv, expected);
            assert_eq!(modern_uv, [1.0 - expected[0], 1.0 - expected[1]]);
        }
        assert_ne!(
            original.faces[BlockFace::Up as usize],
            original.faces[BlockFace::North as usize]
        );
    }
}
