//! Sparse authored PBR terrain pages shared by local Enhanced packs and server texture sets.

use crate::{TextureArray, TextureRef};

/// Authored Enhanced texture layers. The arrays are sparse: `texture_refs`
/// maps carrier references to authored layers, with sorted material-specific overrides
/// when deduplicated carrier images need different authored surfaces.
/// Missing entries keep the generated material fallback, so a pack can ship
/// PBR data for only the blocks it actually authors.
#[derive(Clone, Debug)]
pub struct EnhancedTextureAssets {
    pub color_pages: [TextureArray; 2],
    pub normal_pages: [TextureArray; 2],
    pub mer_pages: [TextureArray; 2],
    pub texture_refs: Box<[u32]>,
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
