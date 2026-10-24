use super::*;

/// The fixture's terrain catalog plus `bindings` as its Cinnabar material bindings.
fn bound_stack(bindings: &Value) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let terrain =
        json!({"texture_data":{"state":{"textures":["textures/base_a","textures/base_b"]}}});
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in [
        ("manifest.json", manifest.to_string().into_bytes()),
        (
            "textures/terrain_texture.json",
            terrain.to_string().into_bytes(),
        ),
        ("textures/base_a.png", png(16, 16, |_, _| [9, 9, 9, 128])),
        ("textures/base_b.png", png(16, 16, |_, _| [9, 9, 9, 128])),
        (
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            bindings.to_string().into_bytes(),
        ),
    ] {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            ),
        ]),
    ))
}

/// Overlay-local face materials each fixture visual is rebound to.
fn rebound(binding: Value) -> Vec<(u32, Vec<u32>)> {
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:glass": {"material": {"states": ["Blending", "DisableCulling"]}}},
        "bindings": [binding]
    });
    let compiled = compile(&bound_stack(&document), &base(), &keys());
    let mut rebound: Vec<_> = compiled
        .overlay
        .face_overrides
        .iter()
        .map(|(visual, faces)| {
            let flags = faces
                .iter()
                .flatten()
                .map(|&material| compiled.overlay.materials[material as usize].flags)
                .collect();
            (*visual, flags)
        })
        .collect();
    rebound.sort_unstable();
    rebound
}

/// The pinned registry record the fixture's first visual stands for.
fn first_record() -> assets::RegistryRecord {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .into_vec()
    .swap_remove(0)
}

#[test]
fn a_vanilla_block_binding_blends_only_that_blocks_cube_faces() {
    let record = first_record();
    let rebound = rebound(json!({"block": record.name.as_ref(), "material": "example:glass"}));
    assert_eq!(rebound.len(), 1, "only the bound visual is rebound");
    assert_eq!(rebound[0].0, 0);
    assert_eq!(rebound[0].1.len(), 6);
    assert!(
        rebound[0]
            .1
            .iter()
            .all(|flags| flags & assets::MATERIAL_FLAG_ALPHA_BLEND != 0)
    );
}

#[test]
fn vanilla_bindings_for_other_blocks_states_or_instances_change_nothing() {
    let record = first_record();
    for binding in [
        json!({"block": "minecraft:not_a_block", "material": "example:glass"}),
        json!({"block": record.name.as_ref(), "states": {"no_such_state": 1},
               "material": "example:glass"}),
        json!({"block": record.name.as_ref(), "material_instance": "up",
               "material": "example:glass"}),
    ] {
        assert!(rebound(binding).is_empty());
    }
}

#[test]
fn vanilla_block_bindings_give_their_faces_glow_masks() {
    let record = first_record();
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:lamp": {
            "textures": {"glow_mask": "state"},
            "parameters": {"strength": {"scalar": 0.5}}
        }},
        "bindings": [{"block": record.name.as_ref(), "material": "example:lamp"}]
    });
    let compiled = compile(&bound_stack(&document), &base(), &keys());
    let (_, faces) = compiled
        .overlay
        .face_overrides
        .iter()
        .find(|(visual, _)| *visual == 0)
        .expect("the bound visual is rebound");
    for &material in faces.iter().flatten() {
        let material = compiled.overlay.materials[material as usize];
        assert_ne!(material.glow_mask, assets::NO_MASK);
        assert_eq!(f32::from_bits(material.mask_strength), 0.5);
    }
}

/// A one-document stack binding the fixture's first registry block to `material`.
fn model_binding(material: Value) -> LayeredPackView {
    let record = first_record();
    bound_stack(&json!({
        "format_version": "1.0.0",
        "materials": {"example:bound": material},
        "bindings": [{"block": record.name.as_ref(), "material": "example:bound"}]
    }))
}

/// The quads of every template part `template` selects, in order.
fn template_quads(runtime: &RuntimeAssets, template: u32) -> Vec<assets::ModelQuad> {
    assets::model_template_parts(runtime.model_templates(), template)
        .unwrap()
        .iter()
        .flat_map(|part| {
            runtime.model_quads()[part.quad_start as usize..][..part.quad_count as usize].to_vec()
        })
        .collect()
}

#[test]
fn a_vanilla_model_binding_redraws_only_that_blocks_quads_with_its_method() {
    let base = super::exclusions::model(VisualKind::Model, &[(0, 2)], 1);
    let glass = json!({"material": {"states": ["Blending", "DisableCulling"]}});
    let compiled = compile(&model_binding(glass), &base, &keys());
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let bound = runtime.resolve(NetworkIdMode::Sequential, 0);
    let other = runtime.resolve(NetworkIdMode::Sequential, 1);
    assert_ne!(
        bound.model_template(),
        base.resolve(NetworkIdMode::Sequential, 0).model_template()
    );
    assert_eq!(
        other,
        base.resolve(NetworkIdMode::Sequential, 1),
        "other blocks keep their model"
    );
    let quads = template_quads(&runtime, bound.model_template().unwrap());
    assert_eq!(quads.len(), 2);
    for quad in quads {
        assert_ne!(
            runtime.material(quad.material).flags & assets::MATERIAL_FLAG_ALPHA_BLEND,
            0
        );
        assert_ne!(quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
    }
    let base_quads = template_quads(&runtime, other.model_template().unwrap());
    assert!(base_quads.iter().all(|quad| quad.material == 1));
}

#[test]
fn a_vanilla_model_binding_masks_its_pot_contents_and_world_model() {
    let base = super::exclusions::model(
        VisualKind::Cross,
        &[(assets::MODEL_TEMPLATE_FLAG_POTTED_PLANT, 1), (0, 1)],
        1,
    );
    let lamp = json!({
        "textures": {"glow_mask": "state"},
        "parameters": {"strength": {"scalar": 0.5}}
    });
    let compiled = compile(&model_binding(lamp), &base, &keys());
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let pot = runtime
        .potted_model_template(NetworkIdMode::Sequential, 0)
        .expect("the copy keeps its pot contents");
    let world = runtime
        .resolve(NetworkIdMode::Sequential, 0)
        .model_template()
        .unwrap();
    assert_eq!(pot + 1, world);
    for quad in template_quads(&runtime, pot)
        .into_iter()
        .chain(template_quads(&runtime, world))
    {
        let material = runtime.material(quad.material);
        assert_ne!(material.glow_mask, assets::NO_MASK);
        assert_eq!(f32::from_bits(material.mask_strength), 0.5);
        assert_eq!(
            quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED,
            0,
            "opaque is one-sided"
        );
    }
}

#[test]
fn a_vanilla_stair_binding_copies_its_whole_topology_group() {
    let stair = (assets::MODEL_TEMPLATE_FLAG_STAIR, 1);
    let base = super::exclusions::model_at(VisualKind::Model, &[stair; 5], 1, 0);
    let glass = json!({"material": {"states": ["Blending"]}});
    let compiled = compile(&model_binding(glass), &base, &keys());
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let head = runtime
        .resolve(NetworkIdMode::Sequential, 0)
        .model_template()
        .unwrap();
    for shape in head..head + assets::STAIR_TEMPLATE_COUNT {
        let template = runtime.model_templates()[shape as usize];
        assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_STAIR);
        for quad in template_quads(&runtime, shape) {
            assert_ne!(
                runtime.material(quad.material).flags & assets::MATERIAL_FLAG_ALPHA_BLEND,
                0
            );
        }
    }
}

#[test]
fn a_template_override_that_does_not_copy_its_span_is_rejected() {
    let base = super::exclusions::model(VisualKind::Model, &[(0, 2)], 1);
    let glass = json!({"material": {"states": ["Blending"]}});
    let mut overlay = compile(&model_binding(glass), &base, &keys()).overlay;
    let (_, head) = overlay.template_overrides[0];
    overlay.model_templates[head as usize].flags = assets::MODEL_TEMPLATE_FLAG_TORCH;
    assert!(base.with_block_overlay(2, &overlay).is_err());
}
