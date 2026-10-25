use super::*;

/// The fixture's terrain catalog plus `bindings` as its Cinnabar material bindings.
fn bound_stack(bindings: &Value) -> LayeredPackView {
    bound_stack_with(bindings, json!({}), Vec::new())
}

/// [`bound_stack`] with the terrain entries `terrain` and the files `extra` add.
fn bound_stack_with(
    bindings: &Value,
    terrain: Value,
    extra: Vec<(&str, Vec<u8>)>,
) -> LayeredPackView {
    let id = "00000000-0000-0000-0000-000000000001";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
        "modules":[{"type":"resources"}]});
    let mut entries = json!({"state":{"textures":["textures/base_a","textures/base_b"]}});
    entries
        .as_object_mut()
        .unwrap()
        .extend(terrain.as_object().unwrap().clone());
    let terrain = json!({"texture_data": entries});
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
    ]
    .into_iter()
    .chain(extra)
    {
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

#[test]
fn every_vanilla_binding_a_state_matches_counts_as_matched_and_the_last_draws() {
    let record = first_record();
    let document = json!({
        "format_version": "1.0.0",
        "materials": {
            "example:glass": {"material": {"states": ["Blending", "DisableCulling"]}},
            "example:lamp": {"textures": {"glow_mask": "state"}}
        },
        "bindings": [
            {"block": record.name.as_ref(), "material": "example:glass"},
            {"block": record.name.as_ref(), "material": "example:lamp"}
        ]
    });
    let compiled = compile(&bound_stack(&document), &base(), &keys());
    assert_eq!(compiled.gaps.unmatched_material_bindings, 0);
    let (_, faces) = compiled
        .overlay
        .face_overrides
        .iter()
        .find(|(visual, _)| *visual == 0)
        .expect("the bound visual is rebound");
    for &material in faces.iter().flatten() {
        let material = compiled.overlay.materials[material as usize];
        assert_ne!(
            material.glow_mask,
            assets::NO_MASK,
            "the last binding draws"
        );
        assert_eq!(material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
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

#[test]
fn a_cutout_binding_keeps_cauldron_water_blended_and_the_overlay_valid() {
    let water = assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_CAULDRON_WATER;
    let cutout = json!({"material": {"defines": ["ALPHA_TEST"], "states": ["DisableCulling"]}});
    let model = |flags| super::exclusions::model_with(VisualKind::Model, &[(0, 2)], &[1], 0, flags);
    let plain = compile(&model_binding(cutout.clone()), &model(0), &keys());
    let base = model(water);
    let compiled = compile(&model_binding(cutout), &base, &keys());
    assert_eq!(
        compiled.gaps.approximated_materials,
        plain.gaps.approximated_materials + 1,
        "the kept water material counts as approximated"
    );
    let runtime = base
        .with_block_overlay(2, &compiled.overlay)
        .expect("the pack's overlay stays valid");
    let bound = runtime.resolve(NetworkIdMode::Sequential, 0);
    let template = bound.model_template().unwrap();
    assert_ne!(
        template,
        base.resolve(NetworkIdMode::Sequential, 0)
            .model_template()
            .unwrap()
    );
    let quads = template_quads(&runtime, template);
    assert_eq!(quads.len(), 2);
    for quad in quads {
        let material = runtime.material(quad.material);
        assert_eq!(material.flags, water, "the water keeps its base flags");
        assert_eq!(material.glow_mask, assets::NO_MASK);
        assert_eq!(
            quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED,
            0,
            "and its sidedness"
        );
    }
}

#[test]
fn a_vanilla_model_whose_later_quad_fails_admits_none_of_its_materials() {
    let record = first_record();
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:glass": {"material": {"states": ["Blending"]}}},
        "bindings": [{"block": record.name.as_ref(), "material": "example:glass"}]
    });
    let base = super::exclusions::model_with(VisualKind::Model, &[(0, 2)], &[1, 2], 0, 0);
    let compiled = compile(&bound_stack(&document), &base, &second_material_unsourced());
    assert!(
        compiled.overlay.template_overrides.is_empty(),
        "the second quad has no terrain art"
    );
    assert!(
        compiled.overlay.materials[1..]
            .iter()
            .all(|material| material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND == 0),
        "the first quad's bound copy is released"
    );
}

/// The fixture's keys with material 2 naming a terrain key no stack defines.
fn second_material_unsourced() -> MaterialKeys {
    keys_with_sources(None, "absent")
}

/// The fixture's keys with the terrain keys materials 1 and 2 copy their art from replaced;
/// `None` keeps material 1's.
fn keys_with_sources(first: Option<&str>, second: &str) -> MaterialKeys {
    let keys = keys();
    let terrain = keys
        .terrain_entries()
        .map(|(key, value)| (key.into(), value.clone()))
        .collect::<Vec<_>>();
    let materials = keys
        .material_sources()
        .map(|(id, source)| {
            let mut source = source.clone();
            match (id, first) {
                (1, Some(key)) => source.key = key.into(),
                (2, _) => source.key = second.into(),
                _ => {}
            }
            (id, source)
        })
        .collect::<Vec<_>>();
    let blocks = keys.block_sources().to_vec();
    keys.with_sources(terrain, materials, blocks)
}

#[test]
fn a_released_texture_slot_takes_no_texture_set_surfaces_to_its_next_texture() {
    let record = first_record();
    let document = json!({
        "format_version": "1.0.0",
        "materials": {"example:glass": {"material": {"states": ["Blending"]}}},
        "bindings": [{"block": record.name.as_ref(), "material": "example:glass"}]
    });
    let surface = || png(16, 16, |x, _| [128 + x as u8, 128, 255, 255]);
    // Material 1 varies over a texture set and a missing raster, so admitting it fails and
    // releases the set's slot, which material 2 then takes.
    let stack = bound_stack_with(
        &document,
        json!({"copied": {"textures": {"variations": [
            {"path": "textures/copied"}, {"path": "textures/missing"}
        ]}}}),
        vec![
            (
                "textures/copied.texture_set.json",
                json!({"format_version": "1.16.100", "minecraft:texture_set": {
                    "color": "copied", "normal": "copied_normal",
                    "metalness_emissive_roughness": [0, 255, 128]
                }})
                .to_string()
                .into_bytes(),
            ),
            (
                "textures/copied.png",
                png(16, 16, |_, _| [40, 90, 140, 255]),
            ),
            ("textures/copied_normal.png", surface()),
        ],
    );
    let base = super::exclusions::model_with(VisualKind::Model, &[(0, 2)], &[1, 2], 0, 0);
    let compiled = compile(&stack, &base, &keys_with_sources(Some("copied"), "state"));
    assert!(
        compiled.overlay.enhanced.is_none(),
        "the released texture's surfaces do not follow its slot"
    );
    base.with_block_overlay(2, &compiled.overlay)
        .expect("the overlay names only its own slots");
}

#[test]
fn a_blend_binding_on_a_two_sided_cutout_keeps_the_binding() {
    let two_sided = assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_TWO_SIDED;
    let glass = json!({"material": {"states": ["Blending", "DisableCulling"]}});
    let model = |flags| super::exclusions::model_with(VisualKind::Model, &[(0, 2)], &[1], 0, flags);
    let plain = compile(&model_binding(glass.clone()), &model(0), &keys());
    let base = model(two_sided);
    let compiled = compile(&model_binding(glass), &base, &keys());
    assert_eq!(
        compiled.gaps.approximated_materials,
        plain.gaps.approximated_materials
    );
    let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
    let template = runtime
        .resolve(NetworkIdMode::Sequential, 0)
        .model_template()
        .unwrap();
    for quad in template_quads(&runtime, template) {
        let flags = runtime.material(quad.material).flags;
        assert_eq!(
            flags & (assets::MATERIAL_FLAG_ALPHA_BLEND | assets::MATERIAL_FLAG_ALPHA_CUTOUT),
            assets::MATERIAL_FLAG_ALPHA_BLEND
        );
        assert_eq!(flags & assets::MATERIAL_FLAG_TWO_SIDED, 0);
        assert_ne!(
            quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED,
            0,
            "the blended quad is two-sided"
        );
    }
}
