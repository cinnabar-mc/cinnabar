//! Carrier decoding and immutable texture-buffer sharing.

use super::*;
use std::io::Read;

/// What a texture section may hold, checked before each raster is allocated.
#[derive(Clone, Copy)]
pub(super) struct TextureBounds {
    pub(super) count: usize,
    pub(super) pixel_bytes: usize,
}

/// The startup carrier's texture bounds.
const STARTUP_TEXTURES: TextureBounds = TextureBounds {
    count: MAX_EQUIPMENT_TEXTURES,
    pixel_bytes: MAX_EQUIPMENT_PIXEL_BYTES,
};

impl RuntimeEquipmentCatalog {
    /// Validates a carrier and retains its decoded texture pixels.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES || bytes.len() > MAX_EQUIPMENT_CARRIER_BYTES {
            return Err(invalid("equipment carrier size is out of bounds"));
        }
        if bytes[..8] != EQUIPMENT_CARRIER_MAGIC
            || u32::from_le_bytes(field::<4>(bytes, 8)?) != EQUIPMENT_CARRIER_VERSION
        {
            return Err(invalid(
                "unsupported equipment carrier header; rebuild with make assets",
            ));
        }
        let payload_bytes = usize::try_from(u64::from_le_bytes(field::<8>(bytes, 12)?))
            .map_err(|_| invalid("equipment payload size exceeds platform"))?;
        if payload_bytes > MAX_EQUIPMENT_METADATA_BYTES {
            return Err(invalid("equipment metadata exceeds bound"));
        }
        let hash_start = bytes.len() - HASH_BYTES;
        let payload_end = HEADER_BYTES
            .checked_add(payload_bytes)
            .filter(|end| *end <= hash_start)
            .ok_or_else(|| invalid("noncanonical equipment carrier layout"))?;
        if Sha256::digest(&bytes[..hash_start]).as_slice() != &bytes[hash_start..] {
            return Err(invalid("equipment carrier envelope hash mismatch"));
        }
        let payload: EquipmentCatalogPayload =
            serde_json::from_slice(&bytes[HEADER_BYTES..payload_end])
                .map_err(|_| invalid("invalid equipment carrier payload"))?;
        let canonical = serde_json::to_vec(&payload)
            .map_err(|_| invalid("failed to canonicalize equipment payload"))?;
        if canonical.as_slice() != &bytes[HEADER_BYTES..payload_end] {
            return Err(invalid("noncanonical equipment payload encoding"));
        }
        validate(
            &payload.source_manifest_sha256,
            &payload.entity_blob_sha256,
            &payload.bindings,
        )?;
        payload.validate_item_facts()?;
        let section = &bytes[payload_end..hash_start];
        let textures = read_textures(
            &mut section.take(section.len() as u64),
            STARTUP_TEXTURES,
            &mut |_| None,
        )?;
        Ok(Self::from_payload(payload, textures))
    }

    /// A catalog of metadata and textures that have each passed their checks.
    pub(super) fn from_payload(
        payload: EquipmentCatalogPayload,
        textures: Vec<EquipmentTexture>,
    ) -> Self {
        Self {
            source_manifest_sha256: payload.source_manifest_sha256,
            entity_blob_sha256: payload.entity_blob_sha256,
            bindings: Arc::from(payload.bindings),
            item_use_overlays: Arc::from(payload.item_use_overlays),
            item_use: Arc::from(payload.item_use),
            item_attack: Arc::from(payload.item_attack),
            item_food: Arc::from(payload.item_food),
            texture_paths_sha256: texture_paths_sha256(&textures),
            textures: Arc::from(textures),
        }
    }

    /// Shares equal texture buffers without changing catalog metadata or pixel values.
    pub(crate) fn share_texture_pixels(
        &mut self,
        mut shared: impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
    ) -> bool {
        let replacements: Vec<_> = self
            .textures
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                shared(&texture.rgba8)
                    .filter(|pixels| !Arc::ptr_eq(pixels, &texture.rgba8))
                    .map(|pixels| (index, pixels))
            })
            .collect();
        if replacements.is_empty() {
            return false;
        }
        let textures = Arc::make_mut(&mut self.textures);
        for (index, pixels) in replacements {
            textures[index].rgba8 = pixels;
        }
        true
    }
}

impl EquipmentCatalogPayload {
    /// Checks the per-item facts every encoding of a catalog carries beside its bindings.
    pub(super) fn validate_item_facts(&self) -> Result<(), AssetError> {
        validate_item_use(&self.item_use)?;
        attack::validate(&self.item_attack)?;
        use_profiles::validate(&self.item_use_overlays)?;
        food::validate(&self.item_food)
    }
}

/// Reads and validates all of `reader`'s texture records, within `bounds`, each raster straight
/// into its own buffer unless `shared` already holds equal pixels.
pub(super) fn read_textures<R: Read>(
    reader: &mut std::io::Take<R>,
    bounds: TextureBounds,
    shared: &mut impl FnMut(&[u8]) -> Option<Arc<[u8]>>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let count = u32::from_le_bytes(read_field(reader)?) as usize;
    if count > bounds.count {
        return Err(invalid("equipment texture count exceeds bound"));
    }
    let mut textures = Vec::with_capacity(count.min(MAX_EQUIPMENT_TEXTURES));
    let mut pixel_bytes = 0usize;
    for _ in 0..count {
        let name_len = usize::from(u16::from_le_bytes(read_field(reader)?));
        let mut identifier = vec![0; name_len];
        reader
            .read_exact(&mut identifier)
            .map_err(|_| truncated_textures())?;
        let identifier = String::from_utf8(identifier)
            .map_err(|_| invalid("equipment texture identifier is not UTF-8"))?
            .into_boxed_str();
        let width = u16::from_le_bytes(read_field(reader)?);
        let height = u16::from_le_bytes(read_field(reader)?);
        let length = texture_pixel_bytes(width, height)
            .ok_or_else(|| invalid("equipment texture dimensions exceed bound"))?;
        pixel_bytes = pixel_bytes
            .checked_add(length)
            .filter(|total| *total <= bounds.pixel_bytes)
            .ok_or_else(|| invalid("equipment texture pixel budget exceeded"))?;
        if length as u64 > reader.limit() {
            return Err(truncated_textures());
        }
        // One exact allocation that the pixels are then read into, never a staged copy.
        let mut rgba8: Arc<[u8]> = std::iter::repeat_n(0, length).collect();
        reader
            .read_exact(Arc::get_mut(&mut rgba8).expect("a new buffer is unshared"))
            .map_err(|_| truncated_textures())?;
        let rgba8 = shared(&rgba8).unwrap_or(rgba8);
        textures.push(EquipmentTexture {
            identifier,
            width,
            height,
            rgba8,
        });
    }
    if reader.limit() != 0 {
        return Err(invalid("trailing bytes after equipment texture section"));
    }
    validate_texture_entries(&textures, bounds.pixel_bytes)?;
    Ok(textures)
}

/// Reads exactly one fixed-size field of a texture section.
pub(super) fn read_field<const N: usize>(reader: &mut impl Read) -> Result<[u8; N], AssetError> {
    let mut bytes = [0; N];
    reader
        .read_exact(&mut bytes)
        .map_err(|_| truncated_textures())?;
    Ok(bytes)
}

fn truncated_textures() -> AssetError {
    invalid("truncated equipment texture section")
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(offset..offset + N)
        .ok_or_else(|| invalid("truncated equipment carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid equipment carrier field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A texture section declaring `count` records, each of a named `side` by `side` raster,
    /// holding only the first record's fields and no pixels.
    fn section(count: u32, side: u16) -> Vec<u8> {
        let identifier = b"textures/items/oversized";
        let mut section = Vec::new();
        section.extend_from_slice(&count.to_le_bytes());
        section.extend_from_slice(&(identifier.len() as u16).to_le_bytes());
        section.extend_from_slice(identifier);
        section.extend_from_slice(&side.to_le_bytes());
        section.extend_from_slice(&side.to_le_bytes());
        section
    }

    fn read(section: &[u8], bounds: TextureBounds) -> Result<Vec<EquipmentTexture>, AssetError> {
        read_textures(&mut section.take(section.len() as u64), bounds, &mut |_| {
            None
        })
    }

    #[test]
    fn equipment_texture_decode_checks_pixel_budget_before_reading_raster() {
        let error = read(&section(1, crate::MAX_ACTOR_TEXTURE_SIDE), STARTUP_TEXTURES).unwrap_err();
        assert!(error.to_string().contains("pixel budget"));
    }

    #[test]
    fn the_startup_carrier_decoder_keeps_its_count_bound() {
        let count = MAX_EQUIPMENT_TEXTURES as u32 + 1;
        let error = read(&section(count, 1), STARTUP_TEXTURES).unwrap_err();
        assert!(error.to_string().contains("count exceeds bound"));
    }

    #[test]
    fn a_raster_longer_than_its_section_is_refused_before_allocating() {
        let bounds = TextureBounds {
            count: 1,
            pixel_bytes: MAX_PACK_EQUIPMENT_PIXEL_BYTES,
        };
        let (error, allocated) = test_alloc::measure(|| read(&section(1, 4096), bounds));
        assert!(error.unwrap_err().to_string().contains("truncated"));
        assert!(allocated.bytes < 1 << 20, "{} bytes", allocated.bytes);
    }
}
