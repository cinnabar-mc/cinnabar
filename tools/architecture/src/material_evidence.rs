//! Checks the material evidence matrix, its fixtures and its token inventory.

use std::{collections::BTreeSet, fs, path::Path};

const MATRIX: &str = "docs/evidence/material-behavior.tsv";
const INVENTORY: &str = "docs/evidence/material-source-inventory.tsv";
const COLUMNS: [&str; 17] = [
    "id",
    "area",
    "surface",
    "types",
    "default",
    "inheritance",
    "mutation",
    "version",
    "wire",
    "invalid",
    "slots",
    "conditions",
    "target_status",
    "disposition",
    "evidence",
    "fixture",
    "test",
];
const REQUIRED_AREAS: &[&str] = &[
    "material-loading",
    "states-defines",
    "program-selection",
    "runtime-pipeline",
    "texture-arity",
    "block-instances",
    "texture-sets",
    "runtime-selection",
];

/// Repository root, two levels above this crate.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("architecture crate lives beneath the repository root")
}

/// Non-comment rows after the header, each split into its tab-separated fields.
fn rows(text: &str) -> (Vec<&str>, Vec<Vec<&str>>) {
    let mut lines = text.lines().filter(|line| !line.starts_with('#'));
    let header = lines
        .next()
        .expect("evidence table has a header")
        .split('\t')
        .collect();
    (
        header,
        lines.map(|line| line.split('\t').collect()).collect(),
    )
}

/// Whole identifier-like tokens of one cell, so a short key cannot match inside a longer one.
fn tokens(cell: &str) -> impl Iterator<Item = &str> {
    cell.split(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '+' | '-' | ':'))
    })
    .filter(|token| !token.is_empty())
}

/// Resolves a fixture anchor: an RFC 6901 pointer for JSON, or a byte string for hex NBT.
fn fixture_has_anchor(path: &Path, anchor: &str) -> bool {
    let text = fs::read_to_string(path).expect("material fixture is readable");
    if path.extension().is_some_and(|extension| extension == "hex") {
        let bytes = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .flat_map(str::split_whitespace)
            .map(|byte| u8::from_str_radix(byte, 16).expect("hex fixture holds bytes"))
            .collect::<Vec<_>>();
        return bytes
            .windows(anchor.len())
            .any(|window| window == anchor.as_bytes());
    }
    assert!(
        anchor.starts_with('/'),
        "JSON anchor `{anchor}` is not an RFC 6901 pointer"
    );
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("JSON material fixture parses");
    value.pointer(anchor).is_some()
}

/// Whether `path::name` names an existing Rust function.
fn test_exists(reference: &str) -> bool {
    let Some((path, name)) = reference.rsplit_once("::") else {
        return false;
    };
    fs::read_to_string(root().join(path))
        .is_ok_and(|source| source.contains(&format!("fn {name}(")))
}

#[test]
fn material_matrix_rows_are_complete_and_anchored() {
    let text = fs::read_to_string(root().join(MATRIX)).expect("material matrix is readable");
    let (header, rows) = rows(&text);
    assert_eq!(
        header, COLUMNS,
        "matrix columns are part of the evidence contract"
    );
    let mut ids = BTreeSet::new();
    let mut areas = BTreeSet::new();
    for row in &rows {
        assert_eq!(row.len(), COLUMNS.len(), "row `{}` is incomplete", row[0]);
        assert!(
            row.iter().all(|value| !value.trim().is_empty()),
            "row `{}` has an empty field",
            row[0]
        );
        assert!(ids.insert(row[0]), "duplicate evidence id `{}`", row[0]);
        areas.insert(row[1]);
        assert!(
            matches!(row[12], "matched" | "unresolved" | "older-only"),
            "`{}` has an invalid target status",
            row[0]
        );
        assert!(
            matches!(row[13], "implemented" | "partial" | "unsupported"),
            "`{}` has an invalid disposition",
            row[0]
        );
        let (fixture, anchor) = row[15]
            .split_once('#')
            .unwrap_or_else(|| panic!("fixture for `{}` names no anchor", row[0]));
        let fixture = root().join(fixture);
        assert!(fixture.is_file(), "fixture for `{}` is missing", row[0]);
        assert!(
            fixture_has_anchor(&fixture, anchor),
            "fixture for `{}` lacks anchor `{anchor}`",
            row[0]
        );
        if row[13] == "implemented" {
            assert!(
                test_exists(row[16]),
                "implemented row `{}` must name an existing test, found `{}`",
                row[0],
                row[16]
            );
        }
    }
    for area in REQUIRED_AREAS {
        assert!(areas.contains(area), "matrix is missing area `{area}`");
    }
}

#[test]
fn inventory_tokens_are_disposed_in_the_matrix() {
    let matrix = fs::read_to_string(root().join(MATRIX)).expect("material matrix is readable");
    let (_, rows) = rows(&matrix);
    let covered = rows
        .iter()
        .flat_map(|row| [row[2], row[8], row[11]])
        .flat_map(tokens)
        .collect::<BTreeSet<_>>();
    let inventory =
        fs::read_to_string(root().join(INVENTORY)).expect("token inventory is readable");
    let (header, entries) = self::rows(&inventory);
    assert_eq!(header, ["area", "token"]);
    for entry in entries {
        assert!(
            covered.contains(entry[1]),
            "inventory token `{}` ({}) has no matrix row",
            entry[1],
            entry[0]
        );
    }
}
