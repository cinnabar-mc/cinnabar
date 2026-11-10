//! Entity material list, material file, texture-set and Cinnabar binding collection.
use super::*;

#[test]
fn material_merges_match_declarations_by_name_across_large_files() {
    const COUNT: usize = 20_000;
    let lower = (0..COUNT)
        .map(|index| {
            (
                format!("m{index}:base"),
                json!({"lower": index, "kept": true}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let upper = (0..COUNT)
        .map(|index| (format!("m{index}:other"), json!({"upper": index})))
        .chain([("new".to_owned(), json!({"upper": "new"}))])
        .collect::<serde_json::Map<_, _>>();
    let mut merged = serde_json::Value::Object(lower);
    super::super::patch_material_json(&mut merged, &serde_json::Value::Object(upper));
    let merged = merged.as_object().unwrap();
    assert_eq!(merged.len(), COUNT + 1);
    for index in [0, COUNT / 2, COUNT - 1] {
        assert!(!merged.contains_key(&format!("m{index}:base")));
        assert_eq!(
            merged[&format!("m{index}:other")],
            json!({"lower": index, "kept": true, "upper": index}),
            "the higher spelling keeps the lower fields it does not replace"
        );
    }
    assert_eq!(merged["new"], json!({"upper": "new"}));
}

#[test]
fn the_highest_material_bindings_file_shadows_lower_copies_even_when_invalid() {
    let path = pack_entity_compiler::MATERIAL_BINDINGS_PATH;
    let lower = archive_with(1, path, json!({"format_version":"1.0.0"}));
    let upper = archive_with(2, path, json!({"format_version":"9.9.9"}));
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![upper, lower]),
    ));
    let files = super::super::collect_files(&view, None, None);
    let collected = files
        .iter()
        .filter(|(file, _)| file.as_ref() == path)
        .map(|(_, bytes)| serde_json::from_slice::<serde_json::Value>(bytes).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(collected, [json!({"format_version":"9.9.9"})]);
}

#[test]
fn every_packs_material_list_resolves_into_one_list_naming_its_files() {
    let list = pack_entity_compiler::ENTITY_MATERIAL_LIST_PATH;
    let lower = archive_files(
        1,
        vec![
            (
                list,
                json!([
                    {"path": "materials/shared.material", "+defines": ["LOWER"]},
                    {"path": "materials/custom.json", "tag": "old"}
                ]),
            ),
            ("materials/shared.material", json!({"lower": {}})),
            ("materials/custom.json", json!({"custom": {}})),
            ("materials/unlisted.material", json!({"unlisted": {}})),
        ],
    );
    let upper = archive_files(
        2,
        vec![
            (
                list,
                json!([{"path": "materials/shared.material", "+defines": ["UPPER"]}]),
            ),
            ("materials/shared.material", json!({"upper": {}})),
        ],
    );
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![upper, lower]),
    ));
    let files = super::super::collect_files(&view, None, None)
        .into_iter()
        .filter(|(path, _)| path.starts_with("materials/"))
        .map(|(path, bytes)| {
            (
                path.to_string(),
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let custom = "materials/:listed/1.material";
    assert_eq!(
        files[list],
        json!([
            {"path": "materials/shared.material", "defines": ["UPPER"]},
            {"path": custom, "defines": []}
        ])
    );
    assert_eq!(
        files["materials/shared.material"],
        json!({"lower": {}, "upper": {}})
    );
    assert_eq!(files[custom], json!({"custom": {}}));
    assert_eq!(
        files["materials/unlisted.material"],
        json!({"unlisted": {}})
    );
    assert!(!files.contains_key("materials/custom.json"));
}

#[test]
fn collection_reports_material_list_entries_it_read_in_part() {
    let list = pack_entity_compiler::ENTITY_MATERIAL_LIST_PATH;
    let pack = archive_files(
        1,
        vec![(
            list,
            json!([{"path": "materials/a.material", "tag": "old"}, {"path": ""}]),
        )],
    );
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![pack]),
    ));
    let (_, report) = super::super::collect_files_reporting(&view, None, None);
    assert_eq!((report.list.ignored_tags, report.list.empty_paths), (1, 1));
}

#[test]
fn simple_graphics_loads_the_sad_material_list_and_fancy_the_fancy_one() {
    let pack = archive_files(
        1,
        vec![
            (
                pack_entity_compiler::ENTITY_MATERIAL_LIST_PATH,
                json!([{"path": "materials/fancy.material", "+defines": ["FANCY"]}]),
            ),
            (
                pack_entity_compiler::SIMPLE_ENTITY_MATERIAL_LIST_PATH,
                json!([{"path": "materials/simple.material"}]),
            ),
            ("materials/fancy.material", json!({"fancy": {}})),
            ("materials/simple.material", json!({"simple": {}})),
        ],
    );
    let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![pack]),
    ));
    let list = |fancy| {
        let (files, _) = super::super::collect_files_for_graphics(&view, None, None, fancy);
        files
            .into_iter()
            .find(|(path, _)| path.as_ref() == pack_entity_compiler::ENTITY_MATERIAL_LIST_PATH)
            .map(|(_, bytes)| serde_json::from_slice::<serde_json::Value>(&bytes).unwrap())
    };
    assert_eq!(
        list(true),
        Some(json!([{"path": "materials/fancy.material", "defines": ["FANCY"]}]))
    );
    assert_eq!(
        list(false),
        Some(json!([{"path": "materials/simple.material", "defines": []}]))
    );
}

#[test]
fn entity_texture_sets_validate_colour_images_before_replacing_plain_rasters() {
    let mut plain = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 0, 0, 255]))
        .write_to(&mut plain, image::ImageFormat::Png)
        .unwrap();
    let set = json!({"format_version": "1.16.100", "minecraft:texture_set": {
        "color": "moth_color"
    }})
    .to_string();
    for (extension, format) in [
        ("png", image::ImageFormat::Png),
        ("tga", image::ImageFormat::Tga),
    ] {
        let mut valid = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut valid, format)
            .unwrap();
        let color_path = format!("textures/entity/moth_color.{extension}");
        for (color, corrupt) in [
            (valid.get_ref().as_slice(), false),
            (b"corrupt image".as_slice(), true),
        ] {
            let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[
                ("textures/entity/moth.texture_set.json", set.as_bytes()),
                (&color_path, color),
                ("textures/entity/moth.png", plain.get_ref()),
            ]));
            let (path, bytes) = super::super::texture_file(
                &view,
                None,
                "textures/entity/moth",
                &mut super::super::TextureSetColours::default(),
            )
            .expect("the entity keeps a usable raster");
            assert_eq!(
                path.as_ref(),
                format!(
                    "textures/entity/moth.{}",
                    if corrupt { "png" } else { extension }
                )
            );
            assert_eq!(
                bytes.as_slice(),
                if corrupt {
                    plain.get_ref().as_slice()
                } else {
                    color
                }
            );
        }
    }
}

#[test]
fn entity_textures_resolve_through_a_same_pack_texture_set_to_its_colour() {
    let pixel = |rgba: [u8; 4]| {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    };
    let entity = json!({"format_version": "1.10.0", "minecraft:client_entity": {"description": {
        "identifier": "example:moth", "textures": {"default": "textures/entity/moth"}
    }}});
    for (set, plain_wins) in [
        (
            json!({"format_version": "1.16.100", "minecraft:texture_set": {
            "color": "moth_color", "metalness_emissive_roughness": [0, 0, 255]}}),
            false,
        ),
        (
            json!({"format_version": "9.9.9", "minecraft:texture_set": {"color": "moth_color"}}),
            true,
        ),
        (
            json!({"format_version": "1.16.100", "minecraft:texture_set": {"color": "moth_absent"}}),
            true,
        ),
    ] {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let manifest = json!({"format_version":2,"header":{"uuid":"00000000-0000-0000-0000-000000000009",
            "version":[1,0,0]},"modules":[{"type":"resources"}]});
        for (path, bytes) in [
            ("manifest.json", serde_json::to_vec(&manifest).unwrap()),
            (
                "entity/moth.entity.json",
                serde_json::to_vec(&entity).unwrap(),
            ),
            (
                "textures/entity/moth.texture_set.json",
                serde_json::to_vec(&set).unwrap(),
            ),
            ("textures/entity/moth_color.png", pixel([10, 20, 30, 255])),
            ("textures/entity/moth.png", pixel([200, 0, 0, 255])),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        let archive = protocol::ResourcePackArchive::unencrypted(
            "00000000-0000-0000-0000-000000000009".parse().unwrap(),
            "1.0.0".into(),
            String::new(),
            zip.finish().unwrap().into_inner(),
        );
        let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(vec![archive]),
        ));
        let files = super::super::collect_files(&view, None, None);
        let (_, bytes) = files
            .iter()
            .find(|(path, _)| path.as_ref() == "textures/entity/moth.png")
            .expect("the entity raster is collected");
        let decoded = image::load_from_memory(bytes).unwrap().into_rgba8();
        let expected = if plain_wins {
            [200, 0, 0, 255]
        } else {
            [10, 20, 30, 255]
        };
        assert_eq!(decoded.get_pixel(0, 0).0, expected);
    }
}
