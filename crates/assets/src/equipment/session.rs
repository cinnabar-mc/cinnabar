//! A compiled catalog's section of a session cache entry.
//!
//! Server packs are not held to the startup carrier's counts, so this encoding keeps only the
//! bounds [`RuntimeEquipmentCatalog::from_parts`] applies. The cache entry's digest covers its
//! bytes, and rasters stream rather than being staged beside the catalog that owns them.

use super::decode::{TextureBounds, read_field, read_textures};
use super::*;
use std::io::{Read, Write};

/// A server catalog's only texture bounds; its count follows from the pixel budget.
const SESSION_TEXTURES: TextureBounds = TextureBounds {
    count: usize::MAX,
    pixel_bytes: MAX_PACK_EQUIPMENT_PIXEL_BYTES,
};
/// Bytes in the metadata's length prefix.
const METADATA_LENGTH: usize = std::mem::size_of::<u64>();
/// Fixed bytes of one texture record before its name and pixels.
const TEXTURE_HEADER: usize = 2 + 2 + 2;
/// Bytes in the section's texture count.
const TEXTURE_COUNT: usize = std::mem::size_of::<u32>();

/// A catalog's encoded metadata beside the textures it streams.
pub(crate) struct SessionEquipmentEncoding<'a> {
    metadata: Vec<u8>,
    textures: &'a [EquipmentTexture],
}

impl SessionEquipmentEncoding<'_> {
    /// Exact number of bytes [`Self::write_to`] writes.
    pub(crate) fn encoded_len(&self) -> u64 {
        let textures: usize = self
            .textures
            .iter()
            .map(|texture| TEXTURE_HEADER + texture.identifier.len() + texture.rgba8.len())
            .sum();
        (METADATA_LENGTH + self.metadata.len() + TEXTURE_COUNT + textures) as u64
    }

    /// Writes the metadata then each texture record, from the buffers the catalog owns.
    pub(crate) fn write_to(&self, out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(&(self.metadata.len() as u64).to_le_bytes())?;
        out.write_all(&self.metadata)?;
        write_textures(out, self.textures)
    }
}

impl RuntimeEquipmentCatalog {
    /// Prepares the section [`Self::decode_session`] reads back, without copying pixels.
    pub(crate) fn session_encoding(&self) -> Result<SessionEquipmentEncoding<'_>, AssetError> {
        let payload = EquipmentCatalogPayload {
            item_use_overlays: self.item_use_overlays.to_vec().into(),
            source_manifest_sha256: self.source_manifest_sha256,
            entity_blob_sha256: self.entity_blob_sha256,
            bindings: self.bindings.to_vec().into(),
            item_use: self.item_use.to_vec().into(),
            item_attack: self.item_attack.to_vec(),
            item_food: self.item_food.to_vec(),
        };
        Ok(SessionEquipmentEncoding {
            metadata: serde_json::to_vec(&payload)
                .map_err(|_| invalid("failed to encode equipment payload"))?,
            textures: &self.textures,
        })
    }

    /// Reads all of `section` through a compiled catalog's checks, reading each raster straight
    /// into its own buffer unless `shared` already holds equal pixels.
    pub(crate) fn decode_session<R: Read>(
        section: &mut std::io::Take<R>,
        mut shared: impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
    ) -> Result<Self, AssetError> {
        let length = u64::from_le_bytes(read_field(section)?);
        if length > section.limit() {
            return Err(invalid("truncated equipment session metadata"));
        }
        let mut metadata = Vec::new();
        (&mut *section)
            .take(length)
            .read_to_end(&mut metadata)
            .map_err(|_| invalid("truncated equipment session metadata"))?;
        let payload: EquipmentCatalogPayload = serde_json::from_slice(&metadata)
            .map_err(|_| invalid("invalid equipment session metadata"))?;
        validate_binding_entries(
            &payload.source_manifest_sha256,
            &payload.entity_blob_sha256,
            &payload.bindings,
        )?;
        payload.validate_item_facts()?;
        let textures = read_textures(section, SESSION_TEXTURES, &mut shared)?;
        Ok(Self::from_payload(payload, textures))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(identifier: String, rgba8: &Arc<[u8]>) -> EquipmentTexture {
        EquipmentTexture {
            identifier: identifier.into(),
            width: 1024,
            height: 256,
            rgba8: Arc::clone(rgba8),
        }
    }

    /// Reads `bytes` as one whole session section.
    fn decode(bytes: &[u8]) -> Result<RuntimeEquipmentCatalog, AssetError> {
        RuntimeEquipmentCatalog::decode_session(&mut bytes.take(bytes.len() as u64), |_| None)
    }

    fn encoded(catalog: &RuntimeEquipmentCatalog) -> Vec<u8> {
        let mut bytes = Vec::new();
        let encoding = catalog.session_encoding().unwrap();
        encoding.write_to(&mut bytes).unwrap();
        assert_eq!(encoding.encoded_len(), bytes.len() as u64);
        bytes
    }

    // Past the startup carrier's counts and pixel bytes, which that carrier still refuses. One
    // shared buffer and a pipe keep this far below the size it round-trips.
    #[test]
    fn a_catalog_past_every_startup_bound_streams_back_through_its_section() {
        let bindings: Vec<_> = (0..=MAX_EQUIPMENT_BINDINGS)
            .map(|index| {
                let mut binding = super::super::tests::sample().remove(0);
                binding.identifier = format!("custom:item_{index:05}").into();
                binding
            })
            .collect();
        let rgba8: Arc<[u8]> = vec![7; 1024 * 256 * 4].into();
        let count = (MAX_EQUIPMENT_TEXTURES + 1).max(MAX_EQUIPMENT_PIXEL_BYTES / rgba8.len() + 1);
        let textures: Vec<_> = (0..count)
            .map(|index| raster(format!("textures/items/{index:05}"), &rgba8))
            .collect();
        assert!(count * rgba8.len() > MAX_EQUIPMENT_PIXEL_BYTES);
        let catalog = RuntimeEquipmentCatalog::from_parts([1; 32], bindings, textures).unwrap();
        assert!(
            encode_equipment_catalog_with_textures(
                catalog.source_manifest_sha256(),
                catalog.entity_blob_sha256(),
                catalog.bindings(),
                catalog.textures(),
            )
            .is_err()
        );
        let encoding = catalog.session_encoding().unwrap();
        let length = encoding.encoded_len();
        let (mut reader, mut writer) = std::io::pipe().unwrap();
        let restored = std::thread::scope(|scope| {
            scope.spawn(move || encoding.write_to(&mut writer).unwrap());
            RuntimeEquipmentCatalog::decode_session(&mut (&mut reader).take(length), |bytes| {
                assert_eq!(bytes, &rgba8[..]);
                Some(Arc::clone(&rgba8))
            })
            .unwrap()
        });
        assert_eq!(restored.bindings(), catalog.bindings());
        assert_eq!(restored.textures(), catalog.textures());
        assert_eq!(
            restored.source_manifest_sha256(),
            catalog.source_manifest_sha256()
        );
        assert_eq!(restored.texture_paths_sha256, catalog.texture_paths_sha256);
    }

    #[test]
    fn a_truncated_or_extended_section_does_not_decode() {
        let rgba8: Arc<[u8]> = vec![7; 1024 * 256 * 4].into();
        let catalog = RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            super::super::tests::sample(),
            vec![raster("textures/items/a".into(), &rgba8)],
        )
        .unwrap();
        let bytes = encoded(&catalog);
        assert_eq!(decode(&bytes).unwrap().textures(), catalog.textures());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut extended = bytes;
        extended.push(0);
        assert!(decode(&extended).is_err());
    }

    #[test]
    fn a_section_keeps_the_binding_and_item_fact_checks() {
        let mut bindings = super::super::tests::sample();
        bindings.reverse();
        let unordered = EquipmentCatalogPayload {
            item_use_overlays: Box::new([]),
            source_manifest_sha256: [1; 32],
            entity_blob_sha256: [1; 32],
            bindings: bindings.into(),
            item_use: Box::new([]),
            item_attack: Vec::new(),
            item_food: Vec::new(),
        };
        let metadata = serde_json::to_vec(&unordered).unwrap();
        let mut bytes = (metadata.len() as u64).to_le_bytes().to_vec();
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn startup_carrier_item_facts_survive_a_section() {
        let item_use = [ItemUseDuration {
            identifier: "minecraft:apple".into(),
            ticks: 32,
        }];
        let carrier = encode_equipment_catalog_with_use_profiles(
            [1; 32],
            [2; 32],
            &super::super::tests::sample(),
            &[],
            EquipmentItemFacts {
                item_use: &item_use,
                ..Default::default()
            },
        )
        .unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&carrier).unwrap();
        let restored = decode(&encoded(&catalog)).unwrap();
        assert_eq!(&*restored.item_use, &item_use);
        assert_eq!(restored.entity_blob_sha256(), [2; 32]);
    }
}
