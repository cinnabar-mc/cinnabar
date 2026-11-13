//! The Go core translates Java Edition 1.7.10 chunks with `core/javaedition/legacy/blocks.tsv`.
//! Every Bedrock state it names must exist in the client's pinned block registry, or the
//! client would draw that block as air.

use std::path::Path;

/// One table row: the Java id and metadata, the Bedrock state text and its network hash.
struct Row {
    id: u16,
    meta: u8,
    state: String,
    hash: u32,
}

fn rows() -> Vec<Row> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/javaedition/legacy/blocks.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 4, "malformed row {line:?}");
            Row {
                id: fields[0].parse().expect("block id"),
                meta: fields[1].parse().expect("metadata"),
                state: fields[2].to_owned(),
                hash: fields[3].parse().expect("network hash"),
            }
        })
        .collect()
}

#[test]
fn every_java_legacy_block_maps_to_a_pinned_bedrock_state() {
    let rows = rows();
    assert!(rows.len() > 1000, "table holds only {} rows", rows.len());
    let missing: Vec<String> = rows
        .iter()
        .filter(|row| assets::pinned_block_sequential_id(row.hash).is_none())
        .map(|row| format!("{}:{} {} ({})", row.id, row.meta, row.state, row.hash))
        .collect();
    assert!(
        missing.is_empty(),
        "{} Java 1.7.10 blocks map to states missing from the pinned registry:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn java_legacy_table_covers_ids_in_order_without_repeats() {
    let rows = rows();
    assert_eq!(
        (rows[0].id, rows[0].meta),
        (0, 0),
        "the table starts with air"
    );
    for pair in rows.windows(2) {
        assert!(
            (pair[0].id, pair[0].meta) < (pair[1].id, pair[1].meta),
            "rows {}:{} and {}:{} are out of order",
            pair[0].id,
            pair[0].meta,
            pair[1].id,
            pair[1].meta
        );
    }
}

/// Neighbour-dependent blocks reach the client with their connections in the state. Every
/// variant the translator can produce, listed by its own test, must be a pinned state too.
#[test]
fn every_connected_java_block_state_is_pinned() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../core/javaedition/translate/testdata/connected_states.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let rows: Vec<(&str, u32)> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (state, hash) = line.split_once('\t').expect("state and hash");
            (state, hash.parse().expect("network hash"))
        })
        .collect();
    assert!(
        rows.len() > 500,
        "only {} connected states are listed",
        rows.len()
    );
    let missing: Vec<&str> = rows
        .iter()
        .filter(|(_, hash)| assets::pinned_block_sequential_id(*hash).is_none())
        .map(|(state, _)| *state)
        .collect();
    assert!(
        missing.is_empty(),
        "{} connected states are missing from the pinned registry:\n{}",
        missing.len(),
        missing.join("\n")
    );
}
