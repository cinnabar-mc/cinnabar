use super::*;
use serde_json::json;

/// Writes an original resource fixture beneath its pack root.
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Encodes a small original raster usable as entity art or a mask.
fn pixels() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 8, image::Rgba([90, 40, 20, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Defines an animated actor whose dependencies come from the installed pack.
fn entity(texture: &str) -> Value {
    json!({"format_version":"1.10.0","minecraft:client_entity":{"description":{
        "identifier":"minecraft:example",
        "materials":{"default":"entity_alphatest"},
        "textures":{"default":texture},
        "geometry":{"default":"geometry.fixture"},
        "render_controllers":["controller.render.fixture"],
        "animations":{"idle":"animation.fixture.idle"},
        "scripts":{"animate":["idle"]}
    }}})
}

/// Installs original entity, controller, geometry, animation and artwork fixtures.
fn installed() -> (tempfile::TempDir, VanillaEntityRefs) {
    let root = tempfile::tempdir().unwrap();
    for (path, document) in [
        (
            "entity/nested/unrelated_filename.json",
            entity("textures/entity/base"),
        ),
        (
            "render_controllers/fixture.json",
            json!({"format_version":"1.8.0","render_controllers":{
                "controller.render.fixture":{
                    "geometry":"Geometry.default","textures":["Texture.default"],
                    "materials":[{"*":"Material.default"}]
                }
            }}),
        ),
        (
            "models/entity/fixture.json",
            json!({"format_version":"1.12.0","minecraft:geometry":[{
                "description":{"identifier":"geometry.fixture","texture_width":8,"texture_height":8},
                "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]
            }]}),
        ),
        (
            "animations/fixture.json",
            json!({"format_version":"1.8.0","animations":{
                "animation.fixture.idle":{"loop":true,"bones":{"body":{"rotation":[0,0,0]}}}
            }}),
        ),
    ] {
        write(root.path(), path, &serde_json::to_vec(&document).unwrap());
    }
    write(root.path(), "textures/entity/base.png", &pixels());
    let refs = pack_entity_compiler::compile_vanilla_entity_refs(root.path()).unwrap();
    (root, refs)
}

/// Binds an emissive mask to an existing entity without shipping its definition.
fn bindings() -> Value {
    json!({
        "format_version":"1.0.0",
        "materials":{"test:glow":{
            "base":"entity_alphatest",
            "textures":{"glow_mask":"textures/entity/glow"}
        }},
        "bindings":[{"entity":"minecraft:example","material":"test:glow"}]
    })
}

#[test]
fn material_only_bindings_collect_installed_entities_and_their_dependencies() {
    let (root, refs) = installed();
    let document = bindings().to_string();
    let mask = pixels();
    let view = LayeredPackView::new(crate::test_support::stack(&[
        (
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            document.as_bytes(),
        ),
        ("textures/entity/glow.png", &mask),
    ]));
    let files = collect_files(&view, Some(&refs), Some(root.path()));
    for prefix in ["entity/", "render_controllers/", "models/", "animations/"] {
        assert!(
            files.iter().any(|(path, _)| path.starts_with(prefix)),
            "{prefix}"
        );
    }
    for texture in ["textures/entity/base.png", "textures/entity/glow.png"] {
        assert!(
            files.iter().any(|(path, _)| path.as_ref() == texture),
            "{texture}"
        );
    }
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    assert!(!compiled.entities.render.layers.is_empty());
    for layer in &compiled.entities.render.layers {
        let masks = compiled.entities.render.material_masks[layer.material_masks.unwrap() as usize];
        let glow = masks.glow.expect("installed entity draws the pack's mask");
        assert_eq!(
            compiled.entities.sources[glow as usize].path.as_ref(),
            "textures/entity/glow.png"
        );
    }
    let removed = LayeredPackView::new(crate::test_support::stack(&[]));
    assert!(
        !collect_files(&removed, Some(&refs), Some(root.path()))
            .iter()
            .any(|(path, _)| path.starts_with("entity/"))
    );
}

#[test]
fn material_bindings_keep_authored_entity_definitions_and_deduplicate_targets() {
    let (root, refs) = installed();
    let mut document = bindings();
    document["bindings"].as_array_mut().unwrap().push(json!({
        "entity":"minecraft:example","bone":"body","material":"test:glow"
    }));
    let document = document.to_string();
    let authored = entity("textures/entity/authored").to_string();
    let view = LayeredPackView::new(crate::test_support::stack(&[
        (
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            document.as_bytes(),
        ),
        ("entity/authored.json", authored.as_bytes()),
    ]));
    let files = collect_files(&view, Some(&refs), Some(root.path()));
    let entities = files
        .iter()
        .filter(|(path, _)| path.starts_with("entity/"))
        .collect::<Vec<_>>();
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].0.as_ref(), "entity/authored.json");
    let value = parse_pack_json(&entities[0].1).unwrap();
    assert_eq!(
        value["minecraft:client_entity"]["description"]["textures"]["default"],
        "textures/entity/authored"
    );

    let view = LayeredPackView::new(crate::test_support::stack(&[(
        pack_entity_compiler::MATERIAL_BINDINGS_PATH,
        document.as_bytes(),
    )]));
    assert_eq!(
        collect_files(&view, Some(&refs), Some(root.path()))
            .iter()
            .filter(|(path, _)| path.starts_with("entity/"))
            .count(),
        1
    );
}

#[test]
fn invalid_or_missing_material_targets_collect_no_installed_entities() {
    let (root, refs) = installed();
    for invalid in [true, false] {
        let mut document = bindings();
        if invalid {
            document["format_version"] = json!("99.0.0");
        } else {
            document["bindings"][0]["entity"] = json!("minecraft:missing");
        }
        let document = document.to_string();
        let view = LayeredPackView::new(crate::test_support::stack(&[(
            pack_entity_compiler::MATERIAL_BINDINGS_PATH,
            document.as_bytes(),
        )]));
        assert!(
            !collect_files(&view, Some(&refs), Some(root.path()))
                .iter()
                .any(|(path, _)| path.starts_with("entity/"))
        );
    }
}
