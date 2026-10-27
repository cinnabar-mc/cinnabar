//! Sparse authored PBR terrain pages shared by local Enhanced packs and server texture sets.

use crate::{TextureArray, TextureRef};

/// Authored Enhanced texture layers. The arrays are sparse: `texture_refs`
/// maps carrier references to authored layers, with sorted material-specific overrides
/// when deduplicated carrier images need different authored surfaces.
/// Missing entries keep the generated material fallback, so a pack can ship
/// PBR data for only the blocks it actually authors.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "EnhancedTextureParts")]
pub struct EnhancedTextureAssets {
    pub color_pages: [TextureArray; 2],
    pub normal_pages: [TextureArray; 2],
    pub mer_pages: [TextureArray; 2],
    pub texture_refs: Box<[u32]>,
}

/// The serialized fields of [`EnhancedTextureAssets`], revalidated when read back.
#[derive(serde::Deserialize)]
struct EnhancedTextureParts {
    color_pages: [TextureArray; 2],
    normal_pages: [TextureArray; 2],
    mer_pages: [TextureArray; 2],
    texture_refs: Box<[u32]>,
}

impl TryFrom<EnhancedTextureParts> for EnhancedTextureAssets {
    type Error = &'static str;

    fn try_from(parts: EnhancedTextureParts) -> Result<Self, Self::Error> {
        Self::new(
            parts.color_pages,
            parts.normal_pages,
            parts.mer_pages,
            parts.texture_refs,
        )
        .ok_or("invalid authored Enhanced texture maps")
    }
}

impl EnhancedTextureAssets {
    /// Validates carrier slots followed by sorted material/frame overrides.
    /// `u32::MAX` selects the carrier fallback.
    #[must_use]
    pub fn new(
        color_pages: [TextureArray; 2],
        normal_pages: [TextureArray; 2],
        mer_pages: [TextureArray; 2],
        texture_refs: Box<[u32]>,
    ) -> Option<Self> {
        if !Self::references_are_valid(&texture_refs)
            || color_pages
                .iter()
                .zip(normal_pages.iter())
                .zip(mer_pages.iter())
                .any(|((color, normal), mer)| {
                    color.layers != normal.layers || color.layers != mer.layers
                })
            || Self::reference_values(&texture_refs).any(|texture_ref| {
                if texture_ref == u32::MAX {
                    return false;
                }
                let page = (texture_ref >> 31) as usize;
                let layer = (texture_ref & (crate::texture::PBR_REF_COLOR - 1)) as usize;
                page >= color_pages.len() || layer >= color_pages[page].layers as usize
            })
        {
            return None;
        }
        Some(Self {
            color_pages,
            normal_pages,
            mer_pages,
            texture_refs,
        })
    }

    /// Number of carrier slots before the optional material override triples.
    pub const REFERENCE_BASE_WORDS: usize = crate::MAX_TEXTURE_PAGES * crate::MAX_TEXTURE_LAYERS;
    /// Each sorted override stores material id, carrier reference, and authored reference.
    pub const REFERENCE_OVERRIDE_WORDS: usize = 3;

    /// Encodes material-owned references while leaving unrelated carrier slots on fallback.
    pub fn material_references(entries: std::collections::BTreeMap<(u32, u32), u32>) -> Box<[u32]> {
        let mut words = vec![u32::MAX; Self::REFERENCE_BASE_WORDS];
        words.extend(
            entries
                .into_iter()
                .flat_map(|((material, texture), authored)| [material, texture, authored]),
        );
        words.into_boxed_slice()
    }

    /// Encodes carrier-slot references, one per raw carrier texture reference, for every
    /// material drawing that texture; there are no material overrides.
    pub fn carrier_references(entries: std::collections::BTreeMap<u32, u32>) -> Box<[u32]> {
        let mut words = vec![u32::MAX; Self::REFERENCE_BASE_WORDS];
        for (texture, authored) in entries {
            if let Ok(texture) = TextureRef::from_raw(texture) {
                words[texture.page() as usize * crate::MAX_TEXTURE_LAYERS
                    + texture.layer() as usize] = authored;
            }
        }
        words.into_boxed_slice()
    }

    /// Combines optional local maps with a server's maps. Server-authored carrier slots take
    /// precedence; untouched local slots and local material overrides stay. A page only one
    /// side references is taken from that side; a page both reference is concatenated, the
    /// server's layers after the local ones. `None` when both reference a page at different
    /// sizes, which the shared builders never produce, or when a concatenated page would hold
    /// more layers than a reference can address.
    #[must_use]
    pub fn merged(local: Option<&Self>, server: &Self) -> Option<Self> {
        let Some(local) = local else {
            return Some(server.clone());
        };
        let referenced = |maps: &Self, page: u32| {
            Self::reference_values(&maps.texture_refs)
                .any(|reference| reference != u32::MAX && reference >> 31 == page)
        };
        let mut offsets = [0u32; 2];
        let mut pages: [[Option<TextureArray>; 2]; 3] = Default::default();
        for page in 0..2 {
            let (from_local, from_server) = (referenced(local, page), referenced(server, page));
            let pick = |kind: usize| {
                let arrays = |maps: &Self| -> [TextureArray; 3] {
                    [
                        maps.color_pages[page as usize].clone(),
                        maps.normal_pages[page as usize].clone(),
                        maps.mer_pages[page as usize].clone(),
                    ]
                };
                let (local, server) = (arrays(local), arrays(server));
                match (from_local, from_server) {
                    (true, true) => concatenate(&local[kind], &server[kind]),
                    (false, true) => Some(server[kind].clone()),
                    _ => Some(local[kind].clone()),
                }
            };
            for (kind, slots) in pages.iter_mut().enumerate() {
                slots[page as usize] = Some(pick(kind)?);
            }
            if from_local && from_server {
                offsets[page as usize] = local.color_pages[page as usize].layers;
                if offsets[page as usize] + server.color_pages[page as usize].layers
                    > crate::texture::PBR_REF_COLOR
                {
                    return None;
                }
            }
        }
        let mut words = local.texture_refs.to_vec();
        for (slot, &reference) in server.texture_refs[..Self::REFERENCE_BASE_WORDS]
            .iter()
            .enumerate()
        {
            if reference != u32::MAX {
                let offset = offsets[(reference >> 31) as usize];
                // A shifted layer must stay below the flag bits it shares the word with.
                if (reference & (crate::texture::PBR_REF_COLOR - 1)) + offset
                    >= crate::texture::PBR_REF_COLOR
                {
                    return None;
                }
                words[slot] = reference + offset;
            }
        }
        let [color, normal, mer] = pages.map(|pages| pages.map(|page| page.expect("set above")));
        Self::new(color, normal, mer, words.into_boxed_slice())
    }

    /// Iterates authored values without treating override keys as texture references.
    pub fn reference_values(words: &[u32]) -> impl Iterator<Item = u32> + '_ {
        let (carrier, overrides) = words.split_at(words.len().min(Self::REFERENCE_BASE_WORDS));
        carrier.iter().copied().chain(
            overrides
                .as_chunks::<{ Self::REFERENCE_OVERRIDE_WORDS }>()
                .0
                .iter()
                .map(|entry| entry[2]),
        )
    }

    /// Requires complete, strictly sorted overrides so CPU and shader searches agree.
    fn references_are_valid(words: &[u32]) -> bool {
        if words.len() < Self::REFERENCE_BASE_WORDS {
            return false;
        }
        let overrides = &words[Self::REFERENCE_BASE_WORDS..];
        let (overrides, remainder) = overrides.as_chunks::<{ Self::REFERENCE_OVERRIDE_WORDS }>();
        if !remainder.is_empty() {
            return false;
        }
        let mut previous = None;
        for entry in overrides {
            let key = (entry[0], entry[1]);
            if entry[0] as usize >= crate::MAX_MATERIALS
                || TextureRef::from_raw(entry[1]).is_err()
                || previous.is_some_and(|previous| previous >= key)
            {
                return false;
            }
            previous = Some(key);
        }
        true
    }

    /// Resolves one material's frame; changed server carrier references retain their fallback.
    #[must_use]
    pub fn material_reference(&self, material: u32, texture: TextureRef) -> u32 {
        let key = (material, texture.raw());
        let overrides = &self.texture_refs[Self::REFERENCE_BASE_WORDS..];
        let mut lo = 0;
        let mut hi = overrides.len() / Self::REFERENCE_OVERRIDE_WORDS;
        while lo < hi {
            let middle = lo + (hi - lo) / 2;
            let start = middle * Self::REFERENCE_OVERRIDE_WORDS;
            let entry = &overrides[start..start + Self::REFERENCE_OVERRIDE_WORDS];
            match (entry[0], entry[1]).cmp(&key) {
                std::cmp::Ordering::Less => lo = middle + 1,
                std::cmp::Ordering::Greater => hi = middle,
                std::cmp::Ordering::Equal => return entry[2],
            }
        }
        self.texture_refs
            [texture.page() as usize * crate::MAX_TEXTURE_LAYERS + texture.layer() as usize]
    }

    /// Counts authored layers on pages referenced by the sparse lookup table.
    #[must_use]
    pub fn authored_layer_count(&self) -> usize {
        self.color_pages
            .iter()
            .enumerate()
            .filter(|(page, _)| {
                Self::reference_values(&self.texture_refs)
                    .any(|reference| reference != u32::MAX && (reference >> 31) as usize == *page)
            })
            .map(|(_, color)| color.layers as usize)
            .sum()
    }

    /// Returns optional pages in color, normal, and material binding order.
    pub fn pages(&self) -> [&TextureArray; 6] {
        [
            &self.color_pages[0],
            &self.color_pages[1],
            &self.normal_pages[0],
            &self.normal_pages[1],
            &self.mer_pages[0],
            &self.mer_pages[1],
        ]
    }
}

/// `first`'s layers followed by `second`'s, when their mip chains match.
fn concatenate(first: &TextureArray, second: &TextureArray) -> Option<TextureArray> {
    if first.mips.len() != second.mips.len()
        || first
            .mips
            .iter()
            .zip(second.mips.iter())
            .any(|(first, second)| first.size != second.size)
    {
        return None;
    }
    Some(TextureArray {
        layers: first.layers + second.layers,
        mips: first
            .mips
            .iter()
            .zip(second.mips.iter())
            .map(|(first, second)| crate::texture::TextureMip {
                size: first.size,
                rgba8: [first.rgba8.as_ref(), second.rgba8.as_ref()]
                    .concat()
                    .into_boxed_slice(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod authored_reference_tests {
    use super::*;

    #[test]
    fn malformed_override_keys_cannot_enter_the_gpu_lookup_table() {
        let entries = std::collections::BTreeMap::from([((1, 0), 0), ((2, 0), 1)]);
        let mut words = EnhancedTextureAssets::material_references(entries).into_vec();
        assert!(EnhancedTextureAssets::references_are_valid(&words));
        words.pop();
        assert!(!EnhancedTextureAssets::references_are_valid(&words));
        words.push(1);
        let start = EnhancedTextureAssets::REFERENCE_BASE_WORDS;
        words[start + EnhancedTextureAssets::REFERENCE_OVERRIDE_WORDS] = 1;
        assert!(
            !EnhancedTextureAssets::references_are_valid(&words),
            "duplicate keys are ambiguous"
        );
        words[start + EnhancedTextureAssets::REFERENCE_OVERRIDE_WORDS] = 2;
        for page in 0..crate::MAX_TEXTURE_PAGES as u32 {
            words[start + 1] = TextureRef::new(page, crate::TEXTURE_LAYER_MASK)
                .unwrap()
                .raw();
            assert!(EnhancedTextureAssets::references_are_valid(&words));
        }
        let carrier_bits =
            (crate::TEXTURE_PAGE_MASK << crate::TEXTURE_PAGE_SHIFT) | crate::TEXTURE_LAYER_MASK;
        for bit in 0..u32::BITS {
            let raw = 1 << bit;
            if raw & carrier_bits == 0 {
                words[start + 1] = raw;
                assert!(
                    !EnhancedTextureAssets::references_are_valid(&words),
                    "keys reject invalid carrier bits: {raw:#010x}"
                );
            }
        }
        for page in crate::MAX_TEXTURE_PAGES as u32..=crate::TEXTURE_PAGE_MASK {
            words[start + 1] = page << crate::TEXTURE_PAGE_SHIFT;
            assert!(!EnhancedTextureAssets::references_are_valid(&words));
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    /// A page of `layers` one-texel layers whose bytes all equal `fill`.
    fn page(layers: u32, fill: u8) -> TextureArray {
        TextureArray {
            layers,
            mips: Box::new([crate::texture::TextureMip {
                size: 1,
                rgba8: vec![fill; 4 * layers as usize].into_boxed_slice(),
            }]),
        }
    }

    fn maps(fill: u8, entries: &[(u32, u32)]) -> EnhancedTextureAssets {
        let pages = || [page(1, fill), page(2, fill)];
        EnhancedTextureAssets::new(
            pages(),
            pages(),
            pages(),
            EnhancedTextureAssets::carrier_references(entries.iter().copied().collect()),
        )
        .unwrap()
    }

    #[test]
    fn server_slots_win_and_land_after_local_layers() {
        let local_ref = TextureRef::new(0, 3).unwrap().raw();
        let shared = TextureRef::new(1, 0).unwrap().raw();
        let server_ref = TextureRef::new(1, 5).unwrap().raw();
        let page1 = 1 << 31;
        let local = maps(10, &[(local_ref, page1), (shared, page1 | 1)]);
        let server = maps(20, &[(shared, page1 | 1), (server_ref, page1)]);
        let merged = EnhancedTextureAssets::merged(Some(&local), &server).unwrap();
        assert_eq!(merged.color_pages[1].layers, 4);
        assert_eq!(
            merged.color_pages[0].layers, 1,
            "an unreferenced page is not doubled"
        );
        let slot =
            |texture: u32| merged.material_reference(0, TextureRef::from_raw(texture).unwrap());
        assert_eq!(slot(local_ref), page1, "untouched local slots stay");
        assert_eq!(slot(shared), page1 | 3, "the server wins a shared slot");
        assert_eq!(slot(server_ref), page1 | 2);
        let bytes = &merged.color_pages[1].mips[0].rgba8;
        assert_eq!((bytes[0], bytes[8]), (10, 20));
        assert_eq!(
            EnhancedTextureAssets::merged(None, &server).unwrap(),
            server
        );
    }

    #[test]
    fn a_merge_whose_layers_a_reference_cannot_address_is_refused() {
        let page1 = 1 << 31;
        let maps = |layers: u32, fill: u8, slot: (u32, u32)| {
            let pages = || [page(1, fill), page(layers, fill)];
            EnhancedTextureAssets::new(
                pages(),
                pages(),
                pages(),
                EnhancedTextureAssets::carrier_references([slot].into()),
            )
            .unwrap()
        };
        let local_slot = TextureRef::new(0, 0).unwrap().raw();
        let server_slot = TextureRef::new(1, 0).unwrap().raw();
        let limit = crate::texture::PBR_REF_COLOR;
        let local = maps(limit - 1, 10, (local_slot, page1));
        let fits = maps(1, 20, (server_slot, page1));
        let merged = EnhancedTextureAssets::merged(Some(&local), &fits).unwrap();
        assert_eq!(merged.color_pages[1].layers, limit);
        assert_eq!(
            merged.material_reference(0, TextureRef::from_raw(server_slot).unwrap()),
            page1 | (limit - 1)
        );
        let overflows = maps(2, 20, (server_slot, page1 | 1));
        assert_eq!(
            EnhancedTextureAssets::merged(Some(&local), &overflows),
            None
        );
    }

    #[test]
    fn serialized_maps_are_revalidated() {
        let server = maps(20, &[(TextureRef::new(1, 0).unwrap().raw(), 1 << 31)]);
        let json = serde_json::to_string(&server).unwrap();
        let read: EnhancedTextureAssets = serde_json::from_str(&json).unwrap();
        assert_eq!(read, server);
        let mut broken: serde_json::Value = serde_json::from_str(&json).unwrap();
        broken["normal_pages"][1]["layers"] = 7.into();
        assert!(serde_json::from_value::<EnhancedTextureAssets>(broken).is_err());
    }
}
