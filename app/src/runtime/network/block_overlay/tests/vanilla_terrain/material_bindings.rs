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
