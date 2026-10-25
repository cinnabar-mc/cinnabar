use super::*;

const GEOMETRY: &str = "geometry.test_spear";

/// Encodes original single-colour artwork.
fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba([40, 120, 110, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// A client thrown trident in the shape vanilla ships: named texture and model, no controller.
fn entity() -> Value {
    json!({"format_version": "1.8.0", "minecraft:client_entity": {"description": {
        "identifier": NativeEntityClass::ThrownTrident.identifier().unwrap(),
        "textures": {"default": "textures/entity/spear"},
        "geometry": {"default": GEOMETRY},
    }}})
}

fn pack(entity: Value) -> Vec<(Box<str>, Vec<u8>)> {
    let geometry = json!({"format_version": "1.12.0", "minecraft:geometry": [{
        "description": {"identifier": GEOMETRY, "texture_width": 32, "texture_height": 32},
        "bones": [{"name": "pole", "pivot": [0, 24, 0], "cubes": [
            {"origin": [-0.5, 0, -0.5], "size": [1, 24, 1], "uv": [0, 0]}
        ]}],
    }]});
    vec![
        (
            "entity/thrown_trident.entity.json".into(),
            serde_json::to_vec(&entity).unwrap(),
        ),
        (
            "models/entity/spear.geo.json".into(),
            serde_json::to_vec(&geometry).unwrap(),
        ),
        ("textures/entity/spear.png".into(), png()),
    ]
}

/// The thrown trident's controller, its model candidates and whether generated sources exist.
fn compiled_rig(entity: Value) -> (Option<u32>, Vec<String>, bool) {
    let compiled = crate::compile_actor_pack(pack(entity))
        .unwrap()
        .expect("thrown trident pack");
    let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap();
    let generated = runtime
        .sources()
        .iter()
        .any(|source| source.path.contains("_cinnabar"));
    let symbol = runtime.symbols().iter().position(|symbol| {
        symbol.kind == EntityAssetKind::Entity
            && Some(symbol.identifier.as_ref()) == NativeEntityClass::ThrownTrident.identifier()
    });
    let Some(rig) = runtime
        .rig_bindings()
        .iter()
        .find(|rig| Some(rig.entity_symbol as usize) == symbol)
    else {
        return (None, Vec::new(), generated);
    };
    let models = (rig.first_geometry..rig.first_geometry + u32::from(rig.geometry_count))
        .map(|index| {
            let binding = &runtime.rig_geometries()[index as usize];
            runtime.geometries()[binding.geometry as usize]
                .identifier
                .to_string()
        })
        .collect();
    (rig.render_controller, models, generated)
}

#[test]
fn controllerless_thrown_tridents_draw_their_own_model() {
    let (controller, models, generated) = compiled_rig(entity());
    assert!(
        controller.is_some(),
        "the native renderer needs a body route"
    );
    assert_eq!(models, [GEOMETRY]);
    assert!(generated);
}

#[test]
fn authored_render_controllers_keep_their_own_thrown_trident() {
    let mut entity = entity();
    entity["minecraft:client_entity"]["description"]["render_controllers"] =
        json!(["controller.render.custom_trident"]);
    let (_, _, generated) = compiled_rig(entity);
    assert!(!generated, "authored thrown tridents are not replaced");
}
