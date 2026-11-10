use super::*;

/// Failed faces keep the diagnostic material while their cube still renders.
#[test]
fn cube_keeps_rendering_with_a_failed_face() {
    let instances = [("*", "lucky"), ("up", "absent")]
        .map(|(name, texture)| CustomMaterialInstance::with_authored_defaults(name, texture));
    let chair = block(
        "test:chair",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("minecraft:geometry.full_block".into()),
                materials: Some(instances.into()),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![chair].into(),
        ..CustomBlocks::default()
    };
    let compiled = compile_block_overlay(&view(), &blocks, false, None).expect("overlay");
    assert_eq!(compiled.gaps.missing_textures, 1);
    assert_eq!(compiled.overlay.visuals.len(), 1);
    let visual = compiled.overlay.visuals[0];
    assert_eq!(visual.kind, VisualKind::Cube);
    assert_eq!(
        compiled.overlay.materials[visual.faces[BlockFace::Up as usize] as usize],
        compiled.overlay.materials[assets::DIAGNOSTIC_MATERIAL as usize],
        "only the failed top face degrades"
    );
    assert!(
        compiled.overlay.materials[visual.faces[BlockFace::North as usize] as usize]
            .texture
            .page()
            == 1,
        "healthy faces keep their overlay raster"
    );
    let session = RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .expect("partial cubes apply");
    assert_eq!(
        session.resolve(NetworkIdMode::Sequential, 1).kind(),
        VisualKind::Cube
    );
}

/// Missing textures and refused materials keep diagnostic quads and authored sidedness.
#[test]
fn model_keeps_quads_and_sidedness_with_failed_materials() {
    let view = view();
    let base = RuntimeAssets::diagnostic();
    let full = base
        .with_block_overlay(
            1,
            &assets::BlockOverlay {
                materials: vec![
                    base.material(0);
                    assets::MAX_MATERIALS - base.materials().len() - 1
                ],
                ..Default::default()
            },
        )
        .expect("reserve all but the diagnostic material slot");
    for (base, texture) in [(&base, "absent"), (&full, "lucky")] {
        for (method, two_sided) in [
            (None, false),
            (Some("opaque"), false),
            (Some("alpha_test"), true),
            (Some("alpha_test_to_opaque"), true),
            (Some("double_sided"), true),
            (Some("blend"), true),
            (Some("blend_to_opaque"), true),
        ] {
            let mut instances = materials(texture).unwrap();
            instances[0].render_method = protocol_ids::BlockRenderMethod::from_network(method);
            let statue = block(
                "test:statue",
                1,
                CustomBlockVisuals {
                    base: CustomVisualComponents {
                        geometry: Some("geometry.gen".into()),
                        materials: Some(instances),
                        ..CustomVisualComponents::default()
                    },
                    ..CustomBlockVisuals::default()
                },
            );
            let blocks = CustomBlocks {
                blocks: vec![statue].into(),
                ..CustomBlocks::default()
            };
            let compiled = super::super::compile_block_overlay(
                &view,
                &blocks,
                false,
                None,
                assets::MAX_TEXTURE_LAYERS as u32,
                Some(base),
            )
            .expect("overlay");
            assert_eq!(compiled.gaps.missing_textures, 1);
            assert_eq!(compiled.overlay.visuals.len(), 1);
            assert_eq!(compiled.overlay.materials.len(), 1);
            assert_eq!(compiled.overlay.textures[0].layers, 1);
            let visual = compiled.overlay.visuals[0];
            assert_eq!(visual.kind, VisualKind::Model);
            let template = compiled.overlay.model_templates[visual.model_template as usize];
            assert!(template.quad_count > 0, "failed quads stay in the mesh");
            let quads = &compiled.overlay.model_quads[template.quad_start as usize..]
                [..template.quad_count as usize];
            for quad in quads {
                assert_eq!(quad.material, assets::DIAGNOSTIC_MATERIAL);
                assert_eq!(
                    quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0,
                    two_sided,
                    "{texture} {method:?}"
                );
            }
            let session = base
                .with_block_overlay(1, &compiled.overlay)
                .expect("partial models apply");
            assert_eq!(session.model_quads().len(), quads.len());
            for (published, compiled) in session.model_quads().iter().zip(quads) {
                assert_eq!(published.flags, compiled.flags);
            }
        }
    }
}

fn full_block(instances: Box<[CustomMaterialInstance]>) -> CustomBlocks {
    CustomBlocks {
        blocks: vec![block(
            "test:surface",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("minecraft:geometry.full_block".into()),
                    materials: Some(instances),
                    ..Default::default()
                },
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    }
}

#[test]
fn blended_full_cubes_keep_per_face_layers_on_the_model_path() {
    let mut instances = materials("lucky").unwrap().into_vec();
    instances[0].render_method = protocol_ids::BlockRenderMethod::Blend;
    instances.push(CustomMaterialInstance::with_authored_defaults(
        "up", "lucky",
    ));
    let compiled =
        compile_block_overlay(&view(), &full_block(instances.into()), false, None).unwrap();
    let visual = compiled.overlay.visuals[0];
    assert_eq!(visual.kind, VisualKind::Model);
    let template = compiled.overlay.model_templates[visual.model_template as usize];
    assert_eq!(
        template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
        0,
        "identical blended neighbours keep their shared faces"
    );
    let quads = &compiled.overlay.model_quads[template.quad_start as usize..]
        [..template.quad_count as usize];
    let blends = quads
        .iter()
        .filter(|quad| {
            compiled.overlay.materials[quad.material as usize].flags
                & assets::MATERIAL_FLAG_ALPHA_BLEND
                != 0
        })
        .count();
    assert_eq!(blends, 5, "the opaque top keeps its own layer");
    assert_eq!(compiled.gaps.approximated_materials, 0);
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &compiled.overlay)
        .expect("blended model publishes");
}

#[test]
fn isotropic_instances_randomize_only_their_own_faces() {
    let mut instances = materials("lucky").unwrap().into_vec();
    instances[0].packed_bools |= 2;
    instances.push(CustomMaterialInstance::with_authored_defaults("up", "gen"));
    let compiled =
        compile_block_overlay(&view(), &full_block(instances.into()), false, None).unwrap();
    let faces = compiled.overlay.visuals[0].faces;
    let flags = |face: BlockFace| compiled.overlay.materials[faces[face as usize] as usize].flags;
    assert_ne!(flags(BlockFace::North) & assets::MATERIAL_FLAG_ISOTROPIC, 0);
    assert_eq!(flags(BlockFace::Up) & assets::MATERIAL_FLAG_ISOTROPIC, 0);
}

#[test]
fn fractional_ambient_occlusion_reaches_the_exponent_field() {
    // Approximations are counted per drawn face, as before.
    for (exponent, encoded, approximated) in [(0.8, 80, 0), (0.805, 0, 6), (4.0, 0, 6)] {
        let mut instances = materials("lucky").unwrap();
        instances[0].ambient_occlusion = exponent;
        let compiled = compile_block_overlay(&view(), &full_block(instances), false, None).unwrap();
        let material = compiled.overlay.materials[compiled.overlay.visuals[0].faces[0] as usize];
        assert_eq!(
            material.flags & assets::MATERIAL_LEAF_AO_EXPONENT_MASK,
            encoded << assets::MATERIAL_LEAF_AO_EXPONENT_SHIFT,
            "{exponent}"
        );
        assert_eq!(
            compiled.gaps.approximated_materials, approximated,
            "{exponent}"
        );
    }
}
