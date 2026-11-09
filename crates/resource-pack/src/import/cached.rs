//! Best-effort display metadata for downloaded archives; this does not admit pack content.
use crate::{InstalledPack, MAX_MANIFEST_BYTES};
use std::{
    io::{Read, Seek},
    sync::Arc,
};
use zip::ZipArchive;

/// Display identity and optional encoded thumbnail read without admitting pack content.
pub struct CachedPackMetadata {
    pub pack: InstalledPack,
    pub icon: Option<Arc<[u8]>>,
}

/// Reads the manifest, English name fallback and optional icon without extracting files.
/// Unreadable or oversized metadata returns `None`; the archive can still be listed and deleted.
pub fn cached_pack_metadata(reader: impl Read + Seek) -> Option<CachedPackMetadata> {
    let mut archive = ZipArchive::new(reader).ok()?;
    let manifest = archive
        .file_names()
        .filter(|name| *name == "manifest.json" || name.ends_with("/manifest.json"))
        .min_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)))?
        .to_owned();
    let bytes = read(&mut archive, &manifest, MAX_MANIFEST_BYTES)?;
    let mut metadata = super::read_metadata(&bytes).ok()??;
    let prefix = manifest.strip_suffix("manifest.json")?;
    if let Some(language) = read(
        &mut archive,
        &format!("{prefix}texts/en_US.lang"),
        MAX_MANIFEST_BYTES,
    ) {
        super::localize_metadata(&mut metadata, &language);
    }
    let icon = read(
        &mut archive,
        &format!("{prefix}pack_icon.png"),
        crate::MAX_PACK_ICON_BYTES,
    )
    .map(Arc::from);
    Some(CachedPackMetadata {
        pack: metadata,
        icon,
    })
}

/// Bounds inflated display metadata independently of the size of the pack itself.
fn read<R: Read + Seek>(archive: &mut ZipArchive<R>, name: &str, limit: usize) -> Option<Vec<u8>> {
    let file = archive.by_name(name).ok()?;
    if file.size() > limit as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= limit).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Builds an archive whose manifest may be nested without extracting it to disk.
    fn archive(manifest: &[u8], prefix: &str, icon: Option<&[u8]>) -> Cursor<Vec<u8>> {
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
        if let Some(icon) = icon {
            writer
                .start_file(
                    format!("{prefix}pack_icon.png"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(icon).unwrap();
        }
        writer.finish().unwrap()
    }

    #[test]
    fn nested_archives_resolve_display_names_and_keep_manifest_versions() {
        let manifest = br#"{"format_version":2,"header":{"name":"pack.name","uuid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]},"modules":[{"type":"resources","uuid":"bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee","version":[1,2,3]}]}"#;
        for prefix in ["", "nested/"] {
            let pack = cached_pack_metadata(archive(manifest, prefix, Some(b"encoded thumbnail")))
                .unwrap();
            assert_eq!(pack.pack.name, "Cloud Islands");
            assert_eq!(pack.pack.version, [1, 2, 3]);
            assert_eq!(pack.icon.as_deref(), Some(b"encoded thumbnail".as_slice()));
            let missing = cached_pack_metadata(archive(manifest, prefix, None)).unwrap();
            assert!(missing.icon.is_none());
            let oversized = cached_pack_metadata(archive(
                manifest,
                prefix,
                Some(&vec![0; crate::MAX_PACK_ICON_BYTES + 1]),
            ))
            .unwrap();
            assert_eq!(oversized.pack.name, "Cloud Islands");
            assert!(oversized.icon.is_none());
        }
    }

    #[test]
    fn invalid_or_oversized_metadata_is_optional() {
        assert!(cached_pack_metadata(Cursor::new(b"not a zip")).is_none());
        assert!(cached_pack_metadata(archive(b"invalid json", "", None)).is_none());
        assert!(
            cached_pack_metadata(archive(&vec![b' '; MAX_MANIFEST_BYTES + 1], "", None)).is_none()
        );
    }
}
