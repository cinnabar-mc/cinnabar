//! Imports a Java Edition resource pack by translating it into an ordinary Bedrock pack archive.

use super::{PACK_IMPORT_EXTENSIONS, directory::BoundedArchive, read_entry};
use crate::{
    AdmissionError, LibraryError, MAX_DECLARED_BYTES_PER_PACK, MAX_FILE_BYTES, MAX_MANIFEST_BYTES,
    MAX_PACK_TEXTURE_BYTES,
};
use std::{
    collections::HashMap,
    io::{Cursor, Write},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The read allowance for a Java pack file: metadata and sky properties take the manifest
/// allowance, textures the ordinary per-file one.
fn read_limit(path: &str) -> u64 {
    if path.ends_with(".mcmeta") || path.ends_with(".properties") {
        MAX_MANIFEST_BYTES as u64
    } else {
        MAX_FILE_BYTES
    }
}

/// The Java pack root among indexed archive paths, when the archive holds a Java pack.
pub(super) fn root(paths: &[(usize, String)]) -> Option<String> {
    java_resource_pack::find_root(paths.iter().map(|(_, path)| path.as_str()))
}

/// Translates the Java pack at `root` into Bedrock pack archive bytes named after `source`.
/// Entries are read within [`read_limit`]; unreadable ones are left out. Generated textures stay
/// within the runtime's texture source limit, and the written archive within the import limits.
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
        |path| read_entry(archive, *entries.get(path)?, read_limit(path)).ok(),
        &pack_name(source, root),
        MAX_PACK_TEXTURE_BYTES,
    )
    .map_err(LibraryError::JavaPack)?;
    let mut writer = ZipWriter::new(BoundedArchive::default());
    let mut declared = 0u64;
    for (path, bytes) in translation.files {
        declared = declared
            .checked_add(bytes.len() as u64)
            .filter(|total| *total <= MAX_DECLARED_BYTES_PER_PACK)
            .ok_or(AdmissionError::DeclaredSizeTooLarge)?;
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
            .map_err(|_| AdmissionError::ArchiveTooLarge)?;
        // The only write failure into memory is the bounded archive reaching its size limit.
        writer
            .write_all(&bytes)
            .map_err(|_| AdmissionError::ArchiveTooLarge)?;
    }
    Ok(writer
        .finish()
        .map_err(|_| AdmissionError::ArchiveTooLarge)?
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
