//! Fixtures recorded from the 1.7.10 client, stored gzip-compressed under `fixtures/`.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The fixture directory of this crate.
pub fn directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The decompressed text of a gzip fixture, or `None` when the file is absent.
///
/// # Panics
/// Panics when the file exists but is not valid gzip or UTF-8.
pub fn read(path: &Path) -> Option<String> {
    let compressed = std::fs::read(path).ok()?;
    let mut text = String::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .read_to_string(&mut text)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    Some(text)
}
