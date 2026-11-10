//! Entity material list, material file and Cinnabar binding collection.
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
fn material_list_casing_aliases_retain_one_document_and_the_first_defines() {
    let list = pack_entity_compiler::ENTITY_MATERIAL_LIST_PATH;
    for extension in ["json", "material"] {
        let entries = (0..4096)
            .map(|variant| {
                let name = "abcdefghijkl"
                    .bytes()
                    .enumerate()
                    .map(|(bit, byte)| {
                        char::from(if variant & (1 << bit) == 0 {
                            byte
                        } else {
                            byte.to_ascii_uppercase()
                        })
                    })
                    .collect::<String>();
                json!({
                    "path": format!("materials/{name}.{extension}"),
                    "+defines": [if variant == 0 { "FIRST" } else { "LATER" }]
                })
            })
            .collect::<Vec<_>>();
        let path = format!("materials/ABCDEFGHIJKL.{extension}");
        let document = json!({"shared": {"payload": "x".repeat(4096)}});
        let pack = archive_files(1, vec![(list, json!(entries)), (&path, document.clone())]);
        let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(vec![pack]),
        ));
        let (files, report) = super::super::material_files(&view, true);
        let files = files
            .into_iter()
            .map(|(path, bytes)| {
                (
                    path,
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let key = if extension == "material" {
            "materials/abcdefghijkl.material"
        } else {
            "materials/:listed/0.material"
        };
        assert_eq!(files.len(), 2, "aliases share one retained payload");
        assert_eq!(files[key], document);
        assert_eq!(files[list], json!([{"path": key, "defines": ["FIRST"]}]));
        assert_eq!(report.repeated_paths, 4095);
    }
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
