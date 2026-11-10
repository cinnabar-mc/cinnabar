use std::io::Cursor;

use super::*;

/// One server entity: client definition, geometry, render controller and a solid texture.
fn entity(name: &str, side: u32) -> Vec<(Box<str>, Vec<u8>)> {
    let mut png = Cursor::new(Vec::new());
    RgbaImage::from_pixel(side, side, Rgba([90, 140, 200, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let definition = serde_json::json!({
        "format_version": "1.10.0",
        "minecraft:client_entity": {"description": {
            "identifier": format!("test:{name}"),
            "materials": {"default": "entity_alphatest"},
            "textures": {"default": format!("textures/entity/{name}")},
            "geometry": {"default": format!("geometry.{name}")},
            "render_controllers": [format!("controller.render.{name}")]
        }}
    });
    let geometry = serde_json::json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {
                "identifier": format!("geometry.{name}"),
                "texture_width": 16,
                "texture_height": 16
            },
            "bones": [{"name": "root", "cubes": [{"origin": [0, 0, 0], "size": [4, 4, 4], "uv": [0, 0]}]}]
        }]
    });
    let controller = serde_json::json!({
        "format_version": "1.8.0",
        "render_controllers": {format!("controller.render.{name}"): {
            "geometry": "Geometry.default",
            "materials": [{"*": "Material.default"}],
            "textures": ["Texture.default"]
        }}
    });
    vec![
        (
            format!("entity/{name}.entity.json").into(),
            serde_json::to_vec(&definition).unwrap(),
        ),
        (
            format!("models/entity/{name}.geo.json").into(),
            serde_json::to_vec(&geometry).unwrap(),
        ),
        (
            format!("render_controllers/{name}.json").into(),
            serde_json::to_vec(&controller).unwrap(),
        ),
        (
            format!("textures/entity/{name}.png").into(),
            png.into_inner(),
        ),
    ]
}

#[test]
fn hundreds_of_megabytes_of_billboard_art_keep_full_resolution() {
    // Five 4096 x 4096 billboards decode to 320 MiB, as large server hub art does.
    let names = ["a", "b", "c", "d", "e"].map(|name| format!("billboard_{name}"));
    let files = names
        .iter()
        .flat_map(|name| entity(name, 4096))
        .collect::<Vec<_>>();
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("every billboard compiles");

    assert!(compiled.fallbacks.is_empty(), "{:?}", compiled.fallbacks);
    assert_eq!(compiled.textures.len(), names.len());
    for texture in &compiled.textures {
        assert_eq!((texture.width, texture.height), (4096, 4096));
    }
}

#[test]
fn a_pack_with_thousands_of_textures_keeps_every_entity_texture() {
    // Sources are taken in path order, so this filler art sorts before the entity's texture.
    let mut filler = Cursor::new(Vec::new());
    RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))
        .write_to(&mut filler, image::ImageFormat::Png)
        .unwrap();
    let filler = filler.into_inner();
    let mut files: Vec<(Box<str>, Vec<u8>)> = (0..4_700)
        .map(|index| {
            (
                format!("textures/aaa_filler/{index:05}.png").into(),
                filler.clone(),
            )
        })
        .collect();
    files.extend(entity("npc", 16));
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("the entity compiles");

    assert_eq!(compiled.skipped.over_budget, 0);
    assert!(
        compiled.bindings.iter().any(|binding| {
            &*compiled.entities.symbols[binding.entity_symbol as usize].identifier == "test:npc"
        }),
        "the entity lost its texture; fallbacks: {:?}",
        compiled.fallbacks
    );
}

#[test]
fn a_pack_with_hundreds_of_thousands_of_keyframes_and_channels_keeps_them_all() {
    // Baked server animations: long clips keyed every frame, and clips over many-boned models.
    const BAKED_ANIMATIONS: usize = 60;
    const BAKED_KEYFRAMES: usize = 10_000;
    const POSED_ANIMATIONS: usize = 140;
    const BONES: usize = 2_000;
    let root = serde_json::json!({
        "name": "root",
        "cubes": [{"origin": [0, 0, 0], "size": [4, 4, 4], "uv": [0, 0]}]
    });
    let bones = std::iter::once(root)
        .chain(
            (0..BONES)
                .map(|bone| serde_json::json!({"name": format!("bone_{bone}"), "parent": "root"})),
        )
        .collect::<Vec<_>>();
    let baked = (0..BAKED_KEYFRAMES)
        .map(|frame| {
            (
                format!("{:.2}", frame as f64 / 20.0),
                serde_json::json!([0, frame % 360, 0]),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let posed = (0..BONES)
        .map(|bone| {
            (
                format!("bone_{bone}"),
                serde_json::json!({"rotation": [0, bone % 360, 0]}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let mut files = entity("baked", 16);
    let mut aliases = serde_json::Map::new();
    for (kind, count, bones) in [
        (
            "baked",
            BAKED_ANIMATIONS,
            serde_json::json!({"root": {"rotation": baked}}),
        ),
        ("posed", POSED_ANIMATIONS, serde_json::Value::from(posed)),
    ] {
        for index in 0..count {
            let identifier = format!("animation.{kind}.{index}");
            aliases.insert(format!("{kind}_{index}"), identifier.clone().into());
            let animation = serde_json::json!({
                "format_version": "1.8.0",
                "animations": {identifier: {"loop": true, "bones": bones}}
            });
            files.push((
                format!("animations/{kind}_{index}.animation.json").into(),
                serde_json::to_vec(&animation).unwrap(),
            ));
        }
    }
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut definition["minecraft:client_entity"]["description"];
    description["animations"] = serde_json::json!(aliases);
    description["scripts"] = serde_json::json!({"animate": aliases.keys().collect::<Vec<_>>()});
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    let mut geometry: serde_json::Value = serde_json::from_slice(&files[1].1).unwrap();
    geometry["minecraft:geometry"][0]["bones"] = bones.into();
    files[1].1 = serde_json::to_vec(&geometry).unwrap();

    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("the entity compiles");
    assert!(compiled.fallbacks.is_empty(), "{:?}", compiled.fallbacks);
    let entities = &compiled.entities;
    let clips_of = |kind: &str| {
        entities
            .animation_clips
            .iter()
            .filter(|clip| {
                entities.symbols[clip.symbol as usize]
                    .identifier
                    .starts_with(&format!("animation.{kind}."))
            })
            .map(|clip| {
                &entities.animation_channels[clip.first_channel as usize..]
                    [..clip.channel_count as usize]
            })
            .collect::<Vec<_>>()
    };
    let baked = clips_of("baked");
    assert!(baked.len() >= BAKED_ANIMATIONS);
    for channels in baked {
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].keyframe_count as usize, BAKED_KEYFRAMES);
    }
    let posed = clips_of("posed");
    assert!(posed.len() >= POSED_ANIMATIONS);
    for channels in posed {
        assert_eq!(channels.len(), BONES);
    }

    let (runtime, blob) = assets::RuntimeEntityAssets::from_compiled_encoded(compiled.entities)
        .expect("the catalog keeps every animation");
    let decoded = assets::RuntimeEntityAssets::decode(&blob.expect("the catalog encodes"))
        .expect("a cached catalog decodes again");
    assert_eq!(decoded.animation_clips(), runtime.animation_clips());
    assert_eq!(decoded.animation_channels(), runtime.animation_channels());
    assert_eq!(decoded.animation_keyframes(), runtime.animation_keyframes());
}
