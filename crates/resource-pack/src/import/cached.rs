//! Best-effort display metadata for downloaded archives; this does not admit pack content.
use crate::{InstalledPack, MAX_MANIFEST_BYTES};
use std::io::{Read, Seek};
use zip::ZipArchive;

/// Reads only the manifest and its English display-name fallback, without extracting files.
/// Unreadable or oversized metadata returns `None`; the archive can still be listed and deleted.
pub fn cached_pack_metadata(reader: impl Read + Seek) -> Option<InstalledPack> {
    let mut archive = ZipArchive::new(reader).ok()?;
    let manifest = archive
        .file_names()
        .filter(|name| *name == "manifest.json" || name.ends_with("/manifest.json"))
        .min_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)))?
        .to_owned();
    let bytes = read(&mut archive, &manifest)?;
    let mut metadata = super::read_metadata(&bytes).ok()??;
    let prefix = manifest.strip_suffix("manifest.json")?;
    if let Some(language) = read(&mut archive, &format!("{prefix}texts/en_US.lang")) {
        super::localize_metadata(&mut metadata, &language);
    }
    Some(metadata)
}

/// Bounds inflated display metadata independently of the size of the pack itself.
fn read<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let file = archive.by_name(name).ok()?;
    if file.size() > MAX_MANIFEST_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_MANIFEST_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Builds an archive whose manifest may be nested without extracting it to disk.
    fn archive(manifest: &[u8], prefix: &str) -> Cursor<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                format!("{prefix}manifest.json"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(manifest).unwrap();
        writer
            .start_file(
                format!("{prefix}texts/en_US.lang"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(b"pack.name=Cloud Islands ## display name\n")
            .unwrap();
        writer
            .start_file(
                "unrelated/deeper/manifest.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(b"incidental nested manifest must not replace the pack header")
            .unwrap();
        writer.finish().unwrap()
    }

    #[test]
    fn nested_archives_resolve_display_names_and_keep_manifest_versions() {
        let manifest = br#"{"format_version":2,"header":{"name":"pack.name","uuid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]},"modules":[{"type":"resources","uuid":"bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]}]}"#;
        for prefix in ["", "nested/"] {
            let pack = cached_pack_metadata(archive(manifest, prefix)).unwrap();
            assert_eq!(pack.name, "Cloud Islands");
            assert_eq!(pack.version, [1, 2, 3]);
        }
    }

    #[test]
    fn invalid_or_oversized_metadata_is_optional() {
        assert!(cached_pack_metadata(Cursor::new(b"not a zip")).is_none());
        assert!(cached_pack_metadata(archive(b"invalid json", "")).is_none());
        assert!(cached_pack_metadata(archive(&vec![b' '; MAX_MANIFEST_BYTES + 1], "")).is_none());
    }
}
