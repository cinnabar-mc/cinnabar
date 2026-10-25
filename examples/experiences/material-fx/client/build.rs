//! Writes the declarations of the showcase's `experience.toml` for `include_declarations!`, and
//! the control channel's operations, which the manifest numbers in `# op <index> <name>: ...`
//! comment lines, as `u16` constants named `<NAME>` plus their `COUNT`.

use std::fmt::Write;
use std::path::Path;

const MANIFEST: &str = "../experience.toml";
/// The file in `OUT_DIR` that `src/lib.rs` includes as `operations`.
const OPERATIONS: &str = "operations.rs";

fn main() {
    experience_sdk::declarations::generate(MANIFEST);
    let text = std::fs::read_to_string(MANIFEST)
        .unwrap_or_else(|error| panic!("reading {MANIFEST}: {error}"));
    let mut out = String::new();
    let mut count = 0u16;
    for line in text.lines() {
        let Some(operation) = line.strip_prefix("# op ") else {
            continue;
        };
        let (index, name) = operation
            .split_once(' ')
            .and_then(|(index, rest)| Some((index, rest.split_once(':')?.0)))
            .unwrap_or_else(|| panic!("{MANIFEST}: malformed operation line {line:?}"));
        assert!(
            index.parse::<u16>() == Ok(count),
            "{MANIFEST}: operation {name} is numbered {index}, not {count}"
        );
        assert!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{MANIFEST}: operation name {name:?} is not lower snake case"
        );
        let constant = name.to_ascii_uppercase();
        writeln!(out, "/// `{name}`, operation {count} of `experience.toml`.")
            .and_then(|()| writeln!(out, "pub const {constant}: u16 = {count};"))
            .expect("writing to a string");
        count += 1;
    }
    writeln!(out, "/// How many operations `experience.toml` numbers.")
        .and_then(|()| writeln!(out, "pub const COUNT: u16 = {count};"))
        .expect("writing to a string");
    let path =
        Path::new(&std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join(OPERATIONS);
    std::fs::write(&path, out)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}
