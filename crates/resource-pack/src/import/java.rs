//! Imports a Java Edition resource pack by translating it into an ordinary Bedrock pack archive.

use super::{PACK_IMPORT_EXTENSIONS, read_entry};
use crate::{AdmissionError, LibraryError, MAX_ARCHIVE_BYTES, MAX_FILE_BYTES};
use std::{
    collections::HashMap,
    io::{Cursor, Write},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The Java pack root among indexed archive paths, when the archive holds a Java pack.
pub(super) fn root(paths: &[(usize, String)]) -> Option<String> {
    java_resource_pack::find_root(paths.iter().map(|(_, path)| path.as_str()))
}

/// Translates the Java pack at `root` into Bedrock pack archive bytes named after `source`.
/// Entries are read with the ordinary per-file limit; unreadable ones are left out.
pub(super) fn translate(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    paths: &[(usize, String)],
    root: &str,
    source: &str,
) -> Result<Vec<u8>, LibraryError> {
    let entries: HashMap<String, usize> = paths
        .iter()
        .filter_map(|(index, path)| Some((path.strip_prefix(root)?.to_owned(), *index)))
        .collect();
    let relative: Vec<String> = entries.keys().cloned().collect();
    let translation = java_resource_pack::translate(
        &relative,
        |path| read_entry(archive, *entries.get(path)?, MAX_FILE_BYTES).ok(),
        &pack_name(source, root),
    )
    .map_err(LibraryError::JavaPack)?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut total = 0usize;
    for (path, bytes) in translation.files {
        total = total.saturating_add(bytes.len());
        if total > MAX_ARCHIVE_BYTES {
            return Err(AdmissionError::ArchiveTooLarge.into());
        }
        // PNGs are already compressed; deflating them again only costs time.
        let method = if path.ends_with(".png") {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        writer
            .start_file(
                path,
                SimpleFileOptions::default().compression_method(method),
            )
            .map_err(|_| AdmissionError::MalformedZip)?;
        writer.write_all(&bytes)?;
    }
    Ok(writer
        .finish()
        .map_err(|_| AdmissionError::MalformedZip)?
        .into_inner())
}

/// The pack's display name: its folder inside the archive, or the imported file or folder name
/// without a pack archive extension.
fn pack_name(source: &str, root: &str) -> String {
    if let Some(folder) = root.trim_end_matches('/').rsplit('/').next()
        && !folder.is_empty()
    {
        return folder.to_owned();
    }
    let name = source.rsplit('/').next().unwrap_or(source);
    let stem = name.rsplit_once('.').and_then(|(stem, extension)| {
        PACK_IMPORT_EXTENSIONS
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
            .then_some(stem)
    });
    match stem.unwrap_or(name).trim() {
        "" => "Java Edition pack".to_owned(),
        name => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::pack_name;

    #[test]
    fn names_come_from_the_pack_folder_or_the_imported_file() {
        assert_eq!(pack_name("Faithful 1.8.9.zip", ""), "Faithful 1.8.9");
        assert_eq!(pack_name("Faithful 1.8.9", ""), "Faithful 1.8.9");
        assert_eq!(pack_name("bundle.zip", "inner/Pack/"), "Pack");
        assert_eq!(pack_name("outer.zip/nested.ZIP", ""), "nested");
    }
}
