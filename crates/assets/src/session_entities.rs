//! Immutable server-pack actor artwork shared by compilation and presentation.
use crate::{
    ActorArtworkBinding, ActorPoseMode, ActorTexture, AssetError, RuntimeEntityAssets,
    RuntimeEquipmentCatalog,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::Arc,
};
/// The pack's entity catalog with the artwork of its eligible rigs.
#[derive(Clone, Debug)]
pub struct SessionEntityPack {
    pub assets: Arc<RuntimeEntityAssets>,
    pub textures: Arc<[ActorTexture]>,
    pub bindings: Arc<[ActorArtworkBinding]>,
    /// The pack's attachable bindings and rasters for held and worn items.
    pub equipment: Option<Arc<crate::RuntimeEquipmentCatalog>>,
}

impl SessionEntityPack {
    /// Shares equipment buffers with equal actor pixels while preserving catalog content.
    pub fn with_shared_equipment_pixels(self: Arc<Self>) -> Arc<Self> {
        if self.textures.is_empty() {
            return self;
        }
        let Some(equipment) = self.equipment.as_ref() else {
            return self;
        };
        let mut equipment = equipment.as_ref().clone();
        let shared = {
            let pixels = SharedActorPixels::new(&self.textures);
            equipment.share_texture_pixels(|bytes| pixels.share(bytes))
        };
        if !shared {
            return self;
        }
        let mut next = Arc::unwrap_or_clone(self);
        next.equipment = Some(Arc::new(equipment));
        Arc::new(next)
    }

    /// Bytes [`Self::decode`] reads back; `entity_blob` is the encoding `assets` was built from.
    pub fn encode(&self, entity_blob: &[u8]) -> Result<Vec<u8>, AssetError> {
        let encoding = self.encoding(entity_blob)?;
        let mut bytes = Vec::with_capacity(usize::try_from(encoding.encoded_len()).unwrap_or(0));
        encoding
            .write_to(&mut bytes)
            .expect("writing to memory cannot fail");
        Ok(bytes)
    }

    /// Prepares the sections [`Self::decode_from`] reads back without copying artwork pixels,
    /// so a caller can learn the exact length before streaming them.
    pub fn encoding<'a>(
        &'a self,
        entity_blob: &'a [u8],
    ) -> Result<SessionEntityEncoding<'a>, AssetError> {
        let equipment = self
            .equipment
            .as_deref()
            .map(RuntimeEquipmentCatalog::session_encoding)
            .transpose()?;
        Ok(SessionEntityEncoding {
            pack: self,
            entity_blob,
            equipment,
        })
    }

    /// Decodes each section; the entity section passes its carrier's checks and the equipment
    /// section a compiled catalog's, without the startup carrier's bounds.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        Self::decode_from(&mut &bytes[..], bytes.len() as u64)
    }

    /// Decodes exactly `length` encoded bytes from `reader`, reading artwork pixels straight
    /// into their shared buffers.
    pub fn decode_from(reader: &mut dyn Read, length: u64) -> Result<Self, AssetError> {
        let mut encoded = Sections {
            reader: reader.take(length),
        };
        let missing = || AssetError::InvalidCompiledAssets {
            detail: "session entity pack section is missing".into(),
        };
        let entity_blob = encoded.next()?.ok_or_else(missing)?;
        let entity_blob = encoded.read(entity_blob)?;
        let artwork = encoded.next()?.ok_or_else(missing)?;
        let mut artwork = (&mut encoded.reader).take(artwork);
        let (textures, bindings) = decode_artwork(&mut artwork)
            .filter(|_| artwork.limit() == 0)
            .ok_or_else(|| AssetError::InvalidCompiledAssets {
                detail: "session entity artwork is malformed".into(),
            })?;
        let equipment = match encoded.next()? {
            None => None,
            Some(length) => {
                let pixels = SharedActorPixels::new(&textures);
                Some(Arc::new(RuntimeEquipmentCatalog::decode_session(
                    &mut (&mut encoded.reader).take(length),
                    |bytes| pixels.share(bytes),
                )?))
            }
        };
        if encoded.next()?.is_some() {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "session entity pack has trailing bytes".into(),
            });
        }
        Ok(Self {
            assets: Arc::new(RuntimeEntityAssets::decode(&entity_blob)?),
            textures: textures.into(),
            bindings: bindings.into(),
            equipment,
        })
    }
}

/// A pack's encoded sections, with only the small equipment metadata staged in memory.
pub struct SessionEntityEncoding<'a> {
    pack: &'a SessionEntityPack,
    entity_blob: &'a [u8],
    equipment: Option<crate::equipment::SessionEquipmentEncoding<'a>>,
}

impl SessionEntityEncoding<'_> {
    /// Exact number of bytes [`Self::write_to`] writes.
    #[must_use]
    pub fn encoded_len(&self) -> u64 {
        [
            Some(self.entity_blob.len() as u64),
            Some(artwork_len(&self.pack.textures, &self.pack.bindings)),
            self.equipment
                .as_ref()
                .map(crate::equipment::SessionEquipmentEncoding::encoded_len),
        ]
        .into_iter()
        .flatten()
        .map(|section| SECTION_LENGTH as u64 + section)
        .sum()
    }

    /// Writes each length-prefixed section, streaming the artwork pixels from their buffers.
    pub fn write_to(&self, out: &mut dyn Write) -> std::io::Result<()> {
        out.write_all(&(self.entity_blob.len() as u64).to_le_bytes())?;
        out.write_all(self.entity_blob)?;
        out.write_all(&artwork_len(&self.pack.textures, &self.pack.bindings).to_le_bytes())?;
        write_artwork(out, &self.pack.textures, &self.pack.bindings)?;
        if let Some(equipment) = &self.equipment {
            out.write_all(&equipment.encoded_len().to_le_bytes())?;
            equipment.write_to(out)?;
        }
        Ok(())
    }
}

/// Bytes in each section's length prefix.
const SECTION_LENGTH: usize = std::mem::size_of::<u64>();
/// Fixed bytes of one encoded texture before its pixels.
const TEXTURE_HEADER: usize = 4 + 2 + 2 + 32 + 8;
/// Fixed bytes of one encoded binding before its material name.
const BINDING_HEADER: usize = 7 * 4 + 4;

/// Reads length-prefixed sections from an encoding of known length.
struct Sections<R> {
    reader: std::io::Take<R>,
}

impl<R: Read> Sections<R> {
    /// The next section's length, `None` at the end, or an error for a truncated section.
    /// The section's bytes follow on `reader`.
    fn next(&mut self) -> Result<Option<u64>, AssetError> {
        if self.reader.limit() == 0 {
            return Ok(None);
        }
        let mut length = [0; SECTION_LENGTH];
        self.reader
            .read_exact(&mut length)
            .map_err(|_| truncated())?;
        let length = u64::from_le_bytes(length);
        if length > self.reader.limit() {
            return Err(truncated());
        }
        Ok(Some(length))
    }

    /// Reads a whole section of `length` bytes, which [`Self::next`] bounded.
    fn read(&mut self, length: u64) -> Result<Vec<u8>, AssetError> {
        let mut bytes = Vec::with_capacity(usize::try_from(length).map_err(|_| truncated())?);
        (&mut self.reader)
            .take(length)
            .read_to_end(&mut bytes)
            .map_err(|_| truncated())?;
        if bytes.len() as u64 != length {
            return Err(truncated());
        }
        Ok(bytes)
    }
}

fn truncated() -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: "session entity pack section is truncated".into(),
    }
}

/// Encoded length of the artwork section [`write_artwork`] writes.
fn artwork_len(textures: &[ActorTexture], bindings: &[ActorArtworkBinding]) -> u64 {
    let textures: usize = textures
        .iter()
        .map(|texture| TEXTURE_HEADER + texture.rgba8.len())
        .sum();
    let bindings: usize = bindings
        .iter()
        .map(|binding| BINDING_HEADER + binding.material.len())
        .sum();
    (4 + textures + 4 + bindings) as u64
}

/// Server artwork keeps bindings the vanilla actor carrier would reject, so it is stored as is.
fn write_artwork(
    out: &mut dyn Write,
    textures: &[ActorTexture],
    bindings: &[ActorArtworkBinding],
) -> std::io::Result<()> {
    out.write_all(&(textures.len() as u32).to_le_bytes())?;
    for texture in textures {
        out.write_all(&texture.source.to_le_bytes())?;
        out.write_all(&texture.width.to_le_bytes())?;
        out.write_all(&texture.height.to_le_bytes())?;
        out.write_all(&texture.pixel_sha256)?;
        out.write_all(&(texture.rgba8.len() as u64).to_le_bytes())?;
        out.write_all(&texture.rgba8)?;
    }
    out.write_all(&(bindings.len() as u32).to_le_bytes())?;
    for binding in bindings {
        for value in [
            binding.rig,
            binding.geometry_candidate,
            binding.entity_symbol,
            binding.geometry,
            binding.render_controller,
            binding.texture,
            binding.pose_mode as u32,
        ] {
            out.write_all(&value.to_le_bytes())?;
        }
        out.write_all(&(binding.material.len() as u32).to_le_bytes())?;
        out.write_all(binding.material.as_bytes())?;
    }
    Ok(())
}

/// Reads the artwork section, each texture's pixels straight into its shared buffer.
fn decode_artwork(
    reader: &mut std::io::Take<impl Read>,
) -> Option<(Vec<ActorTexture>, Vec<ActorArtworkBinding>)> {
    fn array<const N: usize>(reader: &mut impl Read) -> Option<[u8; N]> {
        let mut bytes = [0; N];
        reader.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }
    let u32_of = |reader: &mut std::io::Take<_>| array(reader).map(u32::from_le_bytes);
    let texture_count = u32_of(reader)? as usize;
    let mut textures = Vec::with_capacity(texture_count.min(crate::MAX_ACTOR_TEXTURES));
    for _ in 0..texture_count {
        let source = u32_of(reader)?;
        let width = u16::from_le_bytes(array(reader)?);
        let height = u16::from_le_bytes(array(reader)?);
        let pixel_sha256 = array(reader)?;
        let length = u64::from_le_bytes(array(reader)?);
        if length != u64::from(width) * u64::from(height) * 4 || length > reader.limit() {
            return None;
        }
        // One exact allocation that the pixels are then read into, never a staged copy.
        let mut rgba8: Arc<[u8]> = std::iter::repeat_n(0, usize::try_from(length).ok()?).collect();
        reader
            .read_exact(Arc::get_mut(&mut rgba8).expect("a new buffer is unshared"))
            .ok()?;
        textures.push(ActorTexture {
            source,
            width,
            height,
            pixel_sha256,
            rgba8,
        });
    }
    let binding_count = u32_of(reader)? as usize;
    let mut bindings = Vec::with_capacity(binding_count.min(crate::MAX_ACTOR_BINDINGS));
    for _ in 0..binding_count {
        let [
            rig,
            geometry_candidate,
            entity_symbol,
            geometry,
            render_controller,
            texture,
            pose,
        ] = [(); 7].map(|_| u32_of(reader));
        let pose_mode = match pose? {
            0 => ActorPoseMode::CompiledLiteral,
            1 => ActorPoseMode::RestPose,
            _ => return None,
        };
        let length = u64::from(u32_of(reader)?);
        if length > reader.limit() {
            return None;
        }
        let mut material = Vec::new();
        (&mut *reader)
            .take(length)
            .read_to_end(&mut material)
            .ok()?;
        let material = String::from_utf8(material).ok()?.into();
        if usize::try_from(texture?).ok()? >= textures.len() {
            return None;
        }
        bindings.push(ActorArtworkBinding {
            rig: rig?,
            geometry_candidate: geometry_candidate?,
            entity_symbol: entity_symbol?,
            geometry: geometry?,
            render_controller: render_controller?,
            texture: texture?,
            material,
            pose_mode,
        });
    }
    Some((textures, bindings))
}

/// Looks up equal bytes while the actor catalog keeps their immutable buffers alive.
struct SharedActorPixels<'a> {
    by_digest: std::collections::BTreeMap<[u8; 32], Vec<&'a Arc<[u8]>>>,
    addresses: std::collections::BTreeSet<(*const u8, usize)>,
}

impl<'a> SharedActorPixels<'a> {
    /// Indexes digest candidates and allocations already shared by this catalog.
    fn new(textures: &'a [ActorTexture]) -> Self {
        let mut by_digest = std::collections::BTreeMap::<_, Vec<_>>::new();
        let mut addresses = std::collections::BTreeSet::new();
        for texture in textures {
            by_digest
                .entry(texture.pixel_sha256)
                .or_default()
                .push(&texture.rgba8);
            addresses.insert((texture.rgba8.as_ptr(), texture.rgba8.len()));
        }
        Self {
            by_digest,
            addresses,
        }
    }

    /// Exact comparison keeps digest collisions from changing any texture's bytes.
    fn share(&self, bytes: &[u8]) -> Option<Arc<[u8]>> {
        if self.by_digest.is_empty() || self.addresses.contains(&(bytes.as_ptr(), bytes.len())) {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        self.by_digest
            .get(&digest)?
            .iter()
            .find(|candidate| candidate.as_ref() == bytes)
            .map(|pixels| Arc::clone(pixels))
    }
}
