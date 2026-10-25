//! Numbers the showcase's control operations from the `# op <index> <name>: ...` comment lines
//! of its `experience.toml`, for the build scripts of the client part and the material-fx tool.

use std::fmt::Write;
use std::path::Path;

/// Writes `operations.rs` into `out_dir`: a `u16` constant named `<NAME>` per operation line of
/// `manifest`, in order from 0, and their `COUNT`. Panics on a misnumbered or malformed line.
pub fn write(manifest: &Path, out_dir: &Path) {
    let shown = manifest.display();
    let text = std::fs::read_to_string(manifest)
        .unwrap_or_else(|error| panic!("reading {shown}: {error}"));
    let mut out = String::new();
    let mut count = 0u16;
    for line in text.lines() {
        let Some(operation) = line.strip_prefix("# op ") else {
            continue;
        };
        let (index, name) = operation
            .split_once(' ')
            .and_then(|(index, rest)| Some((index, rest.split_once(':')?.0)))
            .unwrap_or_else(|| panic!("{shown}: malformed operation line {line:?}"));
        assert!(
            index.parse::<u16>() == Ok(count),
            "{shown}: operation {name} is numbered {index}, not {count}"
        );
        assert!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{shown}: operation name {name:?} is not lower snake case"
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
    let path = out_dir.join("operations.rs");
    std::fs::write(&path, out)
        .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}
