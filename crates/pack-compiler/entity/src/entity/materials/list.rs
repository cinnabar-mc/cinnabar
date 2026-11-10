//! Entity material lists, as Bedrock loads them.
//!
//! The client-entity material lookup reads the switchable material group, which loads the list
//! at `materials/fancy.json` with fancy graphics and `materials/sad.json` otherwise. Each entry
//! names one material file by `path` and seeds the root material every declaration in that file
//! starts from with the entry's defines. `tag` is no longer supported and is ignored.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// The entity material list the client loads. Cinnabar always draws with fancy graphics, so it
/// reads the fancy list.
pub const ENTITY_MATERIAL_LIST_PATH: &str = "materials/fancy.json";

/// Longest entry `path` Bedrock copies into its fixed path buffer. A longer path leaves the
/// entry's path empty, so it names no file.
pub const MAX_MATERIAL_LIST_PATH_BYTES: usize = 1023;

/// One material file a list loads and the defines its root material starts with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MaterialListEntry {
    pub path: Box<str>,
    pub defines: BTreeSet<Box<str>>,
}

use assets::MaterialListReport;

/// Resolves the list copies in the order the client processes them: entries of each copy in
/// document order, and a file named again keeps the defines of the entry that loaded it first.
/// An array lists its elements; an object lists its member values, as the client iterates a
/// JSON value either way.
pub fn resolve_material_lists<'a>(
    copies: impl IntoIterator<Item = &'a Value>,
) -> (Vec<MaterialListEntry>, MaterialListReport) {
    let mut report = MaterialListReport::default();
    let mut entries = Vec::<MaterialListEntry>::new();
    let mut loaded = BTreeSet::<Box<str>>::new();
    for copy in copies {
        let members: Box<dyn Iterator<Item = &Value>> = match copy {
            Value::Array(values) => Box::new(values.iter()),
            Value::Object(values) => Box::new(values.values()),
            _ => continue,
        };
        for entry in members.filter_map(Value::as_object) {
            if entry.get("tag").is_some_and(|tag| !tag.is_null()) {
                report.ignored_tags += 1;
            }
            let Some(path) = entry
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty() && path.len() <= MAX_MATERIAL_LIST_PATH_BYTES)
            else {
                report.empty_paths += 1;
                continue;
            };
            if !loaded.insert(path.into()) {
                report.repeated_paths += 1;
                continue;
            }
            entries.push(MaterialListEntry {
                path: path.into(),
                defines: entry_defines(entry),
            });
        }
    }
    (entries, report)
}

/// `defines` and `+defines` add to the entry's set and `-defines` then removes from it.
fn entry_defines(entry: &Map<String, Value>) -> BTreeSet<Box<str>> {
    let names = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(Box::<str>::from)
    };
    let mut defines = names("defines")
        .chain(names("+defines"))
        .collect::<BTreeSet<_>>();
    for name in names("-defines") {
        defines.remove(&name);
    }
    defines
}

/// One list document holding `entries` as resolved, which [`resolve_material_lists`] reads back
/// unchanged.
pub fn material_list_document(entries: &[MaterialListEntry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "path": entry.path.as_ref(),
                    "defines": entry.defines.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn defines(entry: &MaterialListEntry) -> Vec<&str> {
        entry.defines.iter().map(AsRef::as_ref).collect()
    }

    #[test]
    fn entries_load_their_path_with_their_defines_and_ignore_tags() {
        let list = json!([
            {"path": "materials/a.material", "tag": "old", "defines": ["A"], "+defines": ["B", "C"], "-defines": ["C"]},
            {"path": "materials/b.json"}
        ]);
        let (entries, report) = resolve_material_lists([&list]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path.as_ref(), "materials/a.material");
        assert_eq!(defines(&entries[0]), ["A", "B"]);
        assert_eq!(entries[1].path.as_ref(), "materials/b.json");
        assert!(entries[1].defines.is_empty());
        assert_eq!(report.ignored_tags, 1);
    }

    #[test]
    fn a_path_of_1024_bytes_or_more_names_no_file() {
        let longest = format!(
            "materials/{}",
            "a".repeat(MAX_MATERIAL_LIST_PATH_BYTES - 10)
        );
        let overlong = format!("{longest}b");
        assert_eq!(overlong.len(), 1024);
        let list = json!([{"path": longest}, {"path": overlong}, {"path": ""}, {"path": 7}, {}]);
        let (entries, report) = resolve_material_lists([&list]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path.len(), MAX_MATERIAL_LIST_PATH_BYTES);
        assert_eq!(report.empty_paths, 4);
    }

    #[test]
    fn the_first_entry_to_name_a_file_keeps_its_defines() {
        let upper = json!([{"path": "materials/a.material", "+defines": ["UPPER"]}]);
        let lower = json!({"one": {"path": "materials/a.material", "+defines": ["LOWER"]},
                           "two": {"path": "materials/c.material"}});
        let (entries, report) = resolve_material_lists([&upper, &lower]);
        assert_eq!(entries.len(), 2);
        assert_eq!(defines(&entries[0]), ["UPPER"]);
        assert_eq!(entries[1].path.as_ref(), "materials/c.material");
        assert_eq!(report.repeated_paths, 1);
    }

    #[test]
    fn a_written_list_reads_back_unchanged() {
        let list = json!([{"path": "materials/a.material", "+defines": ["B", "A"]}]);
        let (entries, _) = resolve_material_lists([&list]);
        let written = material_list_document(&entries);
        assert_eq!(resolve_material_lists([&written]).0, entries);
    }
}
