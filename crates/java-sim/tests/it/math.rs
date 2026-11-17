//! The sine table must equal the game's entry for entry, since every acceleration reads it.

use java_sim::math::sin_table;
use serde_json::Value;

use crate::fixtures;

/// FNV-1a 64 over each entry's bit pattern as four little-endian bytes.
fn table_hash(table: &[f32]) -> u64 {
    table
        .iter()
        .flat_map(|entry| entry.to_bits().to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

#[test]
fn sine_table_matches_the_game() {
    let text = std::fs::read_to_string(fixtures::directory().join("sin.json"))
        .expect("missing fixture: crates/java-sim/fixtures/sin.json");
    let recorded: Value = serde_json::from_str(&text).expect("sin.json");
    let table = sin_table();
    assert_eq!(table.len(), 65536);
    for sample in recorded["samples"].as_array().expect("samples") {
        let index = sample[0].as_u64().expect("index") as usize;
        let bits = sample[1].as_i64().expect("bits") as u32;
        assert_eq!(table[index].to_bits(), bits, "sine table entry {index}");
    }
    let expected: u64 = recorded["fnv1a64"]
        .as_str()
        .expect("hash")
        .parse()
        .expect("decimal hash");
    assert_eq!(table_hash(table), expected, "sine table hash");
}
