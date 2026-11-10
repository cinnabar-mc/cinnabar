//! Immutable logical UI pages and checked dimension-bucket admission.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use sha2::{Digest, Sha256};

use crate::ui::{
    MAX_UI_FIXED_TEXTURE_BYTES, MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, MAX_UI_TEXTURE_SIDE,
    UiRenderRejectReason,
};

mod mipmaps;
use mipmaps::{UiMip, storage_bytes};

/// Buckets bind independently; page count and byte limits bound their residency.
pub const MAX_UI_TEXTURE_BUCKETS: usize = MAX_UI_TEXTURE_LAYERS as usize;
/// Replaceable pages after static UI: models, session glyphs, server UI, local font and particles.
pub const MAX_UI_DYNAMIC_PAGES: usize = UI_PACK_EQUIPMENT_PAGE_OFFSET + MAX_UI_PACK_EQUIPMENT_PAGES;
/// Side length shared by general dynamic UI pages outside original-resolution model slots.
pub const UI_DYNAMIC_PAGE_SIDE: u32 = 256;
/// Fixed dynamic slot carrying the original player skin, not a projected thumbnail.
pub const UI_PLAYER_SKIN_PAGE_OFFSET: usize = 1;
pub const UI_MODEL_ATLAS_PAGE_OFFSET: usize = UI_PLAYER_SKIN_PAGE_OFFSET + 1;
/// Model-source pages for item cubes, equipment, held artwork and actor flames. Version-matched
/// vanilla carriers fill nine pages with mip-aligned item sources and a tenth with flames; the
/// remainder is headroom for optional block meshes before they fall back to thumbnails.
pub const MAX_UI_MODEL_ATLAS_PAGES: usize = 16;
pub const UI_SESSION_ICON_PAGE_OFFSET: usize =
    UI_MODEL_ATLAS_PAGE_OFFSET + MAX_UI_MODEL_ATLAS_PAGES;
/// Bounded session-font allocation, shared by the default and named bitmap fonts.
pub const UI_SESSION_GLYPH_PAGE_OFFSET: usize = UI_SESSION_ICON_PAGE_OFFSET + 1;
pub const UI_SESSION_GLYPH_PAGES: usize = 8;
/// Dynamic pages reserved for server resource-pack UI textures.
pub const UI_SERVER_UI_PAGES: usize = 8;
/// Dynamic pages reserved for a client part's modal textures.
pub const UI_MODAL_UI_PAGES: usize = 4;
/// Dynamic pages reserved for a player mod's screen textures.
pub const UI_MOD_UI_PAGES: usize = 4;
pub const UI_SESSION_GLYPH_PAGE_SIDE: u32 = 512;
pub const UI_MODEL_ATLAS_SIDE: u32 = 512;
/// Model-atlas residency includes every prepared terrain level.
const fn model_atlas_texels() -> usize {
    let mut level = 0;
    let mut texels = 0;
    while level < assets::texture::VANILLA_TERRAIN_MIP_COUNT {
        texels += (UI_MODEL_ATLAS_SIDE >> level).pow(2) as usize;
        level += 1;
    }
    texels
}
/// Dedicated bounded local-font slot, outside the server glyph and UI allocations.
pub const UI_LOCAL_FONT_PAGE_OFFSET: usize = UI_SESSION_GLYPH_PAGE_OFFSET
    + UI_SESSION_GLYPH_PAGES
    + UI_SERVER_UI_PAGES
    + UI_MODAL_UI_PAGES
    + UI_MOD_UI_PAGES;
/// Dedicated original-resolution atlas for particles in player previews.
pub const UI_PARTICLE_PAGE_OFFSET: usize = UI_LOCAL_FONT_PAGE_OFFSET + 1;
/// Recoverable per-stack shield artwork follows the other fixed dynamic producers.
pub const UI_SHIELD_PAGE_OFFSET: usize = UI_PARTICLE_PAGE_OFFSET + 1;
pub const MAX_UI_SHIELD_PAGES: usize = 8;
/// Per-stack trim icons and worn preview sheets share one recoverable atlas.
pub const UI_TRIM_PAGE_OFFSET: usize = UI_SHIELD_PAGE_OFFSET + MAX_UI_SHIELD_PAGES;
pub const MAX_UI_TRIM_PAGES: usize = 4;
/// Session art of the worn armor and held items, apart from the model atlas that base items
/// can fill. Four armor slots and two hands each get a whole page.
pub const UI_PACK_EQUIPMENT_PAGE_OFFSET: usize = UI_TRIM_PAGE_OFFSET + MAX_UI_TRIM_PAGES;
pub const MAX_UI_PACK_EQUIPMENT_PAGES: usize = 6;
/// Shield artwork and pack equipment: whole model-atlas pages that resize within their capacity.
const UI_SESSION_MODEL_PAGES: std::ops::Range<usize> =
    UI_SHIELD_PAGE_OFFSET..UI_PACK_EQUIPMENT_PAGE_OFFSET + MAX_UI_PACK_EQUIPMENT_PAGES;
pub const UI_LOCAL_FONT_PAGE_SIDE: u32 = 512;
/// Dense local icon fonts use coverage texels, keeping the ordinary RGBA page's byte budget.
pub const UI_LOCAL_ICON_FONT_PAGE_SIDE: u32 = UI_LOCAL_FONT_PAGE_SIDE * 2;
const _: () = assert!(UI_LOCAL_ICON_FONT_PAGE_SIDE <= crate::FONT_ATLAS_SIDE);
/// Replaceable full-resolution pages after the small ones, for menu artwork.
pub const MAX_UI_ART_PAGES: usize = 2;
pub const UI_ART_PAGE_SIDE: u32 = 1024;
/// Original-resolution UI images occupy independent, bounded logical slots.
pub const UI_NATIVE_SHEET_PAGE_OFFSET: usize = MAX_UI_DYNAMIC_PAGES + MAX_UI_ART_PAGES;
pub const MAX_UI_NATIVE_SHEET_PAGES: usize = 8;
/// Selected model sources follow fixed producers and share the catalog's layer and byte budgets.
pub const UI_SELECTED_MODEL_PAGE_OFFSET: usize =
    UI_NATIVE_SHEET_PAGE_OFFSET + MAX_UI_NATIVE_SHEET_PAGES;
pub const MAX_UI_NATIVE_SHEET_BYTES: usize = 96 * 1024 * 1024;
/// Virtual sheets are tiled into ordinary device-supported texture extents.
pub const MAX_UI_NATIVE_SHEET_SIDE: u32 = MAX_UI_TEXTURE_SIDE * 4;
/// Growth beyond blank extents of bounded skin, model, session-icon and UI-image slots.
pub const MAX_UI_NATIVE_TEXTURE_GROWTH_BYTES: usize = (render_api::MAX_STANDARD_SKIN_SIDE as usize
    * render_api::MAX_STANDARD_SKIN_SIDE as usize
    + MAX_UI_MODEL_ATLAS_PAGES * model_atlas_texels()
    + MAX_UI_TEXTURE_SIDE as usize * MAX_UI_TEXTURE_SIDE as usize
    + UI_SESSION_GLYPH_PAGES
        * UI_SESSION_GLYPH_PAGE_SIDE as usize
        * UI_SESSION_GLYPH_PAGE_SIDE as usize
    - (MAX_UI_MODEL_ATLAS_PAGES + UI_SESSION_GLYPH_PAGES + 2)
        * UI_DYNAMIC_PAGE_SIDE as usize
        * UI_DYNAMIC_PAGE_SIDE as usize)
    * UiTextureFormat::Rgba8.bytes_per_texel()
    + MAX_UI_NATIVE_SHEET_BYTES
    + (MAX_UI_SHIELD_PAGES + MAX_UI_TRIM_PAGES)
        * (UI_MODEL_ATLAS_SIDE as usize * UI_MODEL_ATLAS_SIDE as usize
            - UI_DYNAMIC_PAGE_SIDE as usize * UI_DYNAMIC_PAGE_SIDE as usize)
        * UiTextureFormat::Rgba8.bytes_per_texel()
    + MAX_UI_PACK_EQUIPMENT_PAGES
        * (model_atlas_texels() - UI_DYNAMIC_PAGE_SIDE as usize * UI_DYNAMIC_PAGE_SIDE as usize)
        * UiTextureFormat::Rgba8.bytes_per_texel()
    + (MAX_UI_TEXTURE_SIDE as usize * MAX_UI_TEXTURE_SIDE as usize
        - UI_DYNAMIC_PAGE_SIDE as usize * UI_DYNAMIC_PAGE_SIDE as usize)
        * UiTextureFormat::Rgba8.bytes_per_texel();

#[derive(Clone, Debug, Eq, PartialEq)]
enum Pixels {
    Owned(Arc<[u8]>),
    Font {
        catalog: Arc<RuntimeFontCatalog>,
        page: usize,
    },
}

/// How a page's texels are stored, and so the format of the bucket it uploads to.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum UiTextureFormat {
    #[default]
    Rgba8,
    /// One alpha byte per texel of a white page; the shader samples it as white with that alpha.
    Coverage,
}

impl UiTextureFormat {
    pub const fn bytes_per_texel(self) -> usize {
        match self {
            Self::Rgba8 => 4,
            Self::Coverage => 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePage {
    dimensions: [u32; 2],
    identity: [u8; 32],
    format: UiTextureFormat,
    pixels: Pixels,
    /// Every texel is zero, as in an unused reserved slot.
    blank: bool,
    repeat_content: Option<[u32; 4]>,
    mips: Arc<[UiMip]>,
}

impl UiTexturePage {
    /// One alpha byte per texel for demand-filled white font pages.
    pub fn coverage(dimensions: [u32; 2], pixels: Arc<[u8]>) -> Result<Self, UiRenderRejectReason> {
        let expected = page_bytes_in(dimensions, UiTextureFormat::Coverage)?;
        if pixels.len() != expected {
            return Err(UiRenderRejectReason::TextureByteLengthInvalid {
                actual: pixels.len(),
                expected,
            });
        }
        Ok(Self {
            dimensions,
            identity: Sha256::digest(&pixels).into(),
            format: UiTextureFormat::Coverage,
            blank: pixels.iter().all(|&byte| byte == 0),
            pixels: Pixels::Owned(pixels),
            repeat_content: None,
            mips: Arc::from([]),
        })
    }
    pub fn owned(dimensions: [u32; 2], pixels: Arc<[u8]>) -> Result<Self, UiRenderRejectReason> {
        let expected = page_bytes(dimensions)?;
        if pixels.len() != expected {
            return Err(UiRenderRejectReason::TextureByteLengthInvalid {
                actual: pixels.len(),
                expected,
            });
        }
        Ok(Self {
            dimensions,
            identity: Sha256::digest(&pixels).into(),
            format: UiTextureFormat::Rgba8,
            blank: pixels.iter().all(|&byte| byte == 0),
            pixels: Pixels::Owned(pixels),
            repeat_content: None,
            mips: Arc::from([]),
        })
    }

    pub fn font(
        catalog: Arc<RuntimeFontCatalog>,
        page: usize,
    ) -> Result<Self, UiRenderRejectReason> {
        let source = catalog
            .pages()
            .get(page)
            .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
        let dimensions = [source.width, source.height];
        let format = match source.pixels {
            assets::FontPixels::Rgba8(_) => UiTextureFormat::Rgba8,
            assets::FontPixels::Coverage(_) => UiTextureFormat::Coverage,
        };
        if source.pixels.bytes().len() != page_bytes_in(dimensions, format)? {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        // A demand page must not alias a fully uploaded page with identical source pixels.
        let mut identity = Sha256::new();
        identity.update(b"font demand atlas");
        identity.update(source.pixels_sha256);
        for side in dimensions {
            identity.update(side.to_le_bytes());
        }
        Ok(Self {
            dimensions,
            identity: identity.finalize().into(),
            format,
            pixels: Pixels::Font { catalog, page },
            blank: false,
            repeat_content: None,
            mips: Arc::from([]),
        })
    }

    /// Records the non-overlapping content of an original-image page, excluding its filter halo.
    pub fn with_repeat_content(mut self, bounds: [u32; 4]) -> Result<Self, UiRenderRejectReason> {
        if bounds[0] >= bounds[2]
            || bounds[1] >= bounds[3]
            || bounds[2] > self.dimensions[0]
            || bounds[3] > self.dimensions[1]
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut identity = Sha256::new();
        identity.update(self.identity);
        for value in bounds {
            identity.update(value.to_le_bytes());
        }
        self.identity = identity.finalize().into();
        self.repeat_content = Some(bounds);
        Ok(self)
    }

    /// The source texels this page owns, separate from overlapping linear-filter neighbors.
    pub fn repeat_content(&self) -> Option<[u32; 4]> {
        self.repeat_content
    }

    /// Font pages reserve a bounded demand atlas; source texels remain in catalog coordinates.
    pub fn font_atlas_side(&self) -> Option<u32> {
        matches!(self.pixels, Pixels::Font { .. }).then(|| {
            self.dimensions[0]
                .max(self.dimensions[1])
                .next_power_of_two()
                .clamp(64, crate::FONT_ATLAS_SIDE)
        })
    }

    /// Returns the GPU extent, which can be smaller than a font's source page.
    pub fn resident_dimensions(&self) -> [u32; 2] {
        self.font_atlas_side()
            .map_or(self.dimensions, |side| [side; 2])
    }

    pub fn pixels(&self) -> &[u8] {
        match &self.pixels {
            Pixels::Owned(pixels) => pixels,
            Pixels::Font { catalog, page } => catalog.pages()[*page].pixels.bytes(),
        }
    }

    /// Adds independently averaged terrain levels while retaining the original source pixels.
    pub fn with_terrain_mips(mut self) -> Result<Self, UiRenderRejectReason> {
        if !self.mips.is_empty() {
            return Ok(self);
        }
        if self.format != UiTextureFormat::Rgba8 || self.font_atlas_side().is_some() {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        self.mips = mipmaps::terrain_levels(self.pixels(), self.dimensions)?;
        if self.mips.is_empty() {
            return Ok(self);
        }
        let mut identity = Sha256::new();
        identity.update(self.identity);
        for mip in self.mips.iter() {
            for side in mip.dimensions {
                identity.update(side.to_le_bytes());
            }
            identity.update(&mip.pixels);
        }
        self.identity = identity.finalize().into();
        Ok(self)
    }

    /// Includes the source level and every prepared smaller level, in upload order.
    pub fn mip_levels(&self) -> impl Iterator<Item = ([u32; 2], &[u8])> {
        std::iter::once((self.dimensions, self.pixels())).chain(
            self.mips
                .iter()
                .map(|mip| (mip.dimensions, mip.pixels.as_ref())),
        )
    }

    /// Number of levels that the GPU texture must retain.
    pub fn mip_level_count(&self) -> u32 {
        self.mips.len() as u32 + 1
    }

    /// Total upload storage across every source level.
    pub fn byte_len(&self) -> usize {
        self.pixels().len() + self.mips.iter().map(|mip| mip.pixels.len()).sum::<usize>()
    }

    pub const fn dimensions(&self) -> [u32; 2] {
        self.dimensions
    }
    pub const fn format(&self) -> UiTextureFormat {
        self.format
    }
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
    /// Whether every texel is zero. Font pages fill on demand and never count as blank.
    pub const fn is_blank(&self) -> bool {
        self.blank
    }
}

fn page_bytes(dimensions: [u32; 2]) -> Result<usize, UiRenderRejectReason> {
    page_bytes_in(dimensions, UiTextureFormat::Rgba8)
}

fn page_bytes_in(
    [width, height]: [u32; 2],
    format: UiTextureFormat,
) -> Result<usize, UiRenderRejectReason> {
    if width == 0 || height == 0 || width > MAX_UI_TEXTURE_SIDE || height > MAX_UI_TEXTURE_SIDE {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(format.bytes_per_texel()))
        .ok_or(UiRenderRejectReason::InvalidTextureExtent)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextureLocation {
    pub bucket: usize,
    pub layer: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextureBucket {
    pub dimensions: [u32; 2],
    pub format: UiTextureFormat,
    pub layers: u32,
    pub mip_level_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePlan {
    buckets: Box<[UiTextureBucket]>,
    locations: Box<[UiTextureLocation]>,
    bytes: usize,
}

impl UiTexturePlan {
    /// Dry plan all pages, including blank reservations, before allocating pixels.
    pub fn new(dimensions: &[[u32; 2]]) -> Result<Self, UiRenderRejectReason> {
        let pages = dimensions
            .iter()
            .map(|&dimensions| (dimensions, UiTextureFormat::Rgba8))
            .collect::<Vec<_>>();
        Self::with_formats(&pages)
    }

    /// Plans pages of mixed storage; each format gets its own buckets.
    pub fn with_formats(
        pages: &[([u32; 2], UiTextureFormat)],
    ) -> Result<Self, UiRenderRejectReason> {
        let levels = pages
            .iter()
            .map(|&(dimensions, format)| (dimensions, format, 1))
            .collect::<Vec<_>>();
        Self::with_levels(&levels)
    }

    /// Groups only pages whose extents, format and complete mip storage agree.
    fn with_levels(
        pages: &[([u32; 2], UiTextureFormat, u32)],
    ) -> Result<Self, UiRenderRejectReason> {
        if pages.is_empty() || pages.len() > MAX_UI_TEXTURE_LAYERS as usize {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut buckets = Vec::<UiTextureBucket>::new();
        let mut locations = Vec::with_capacity(pages.len());
        let mut bytes = 0usize;
        for &(dimensions, format, mip_level_count) in pages {
            bytes = bytes
                .checked_add(storage_bytes(dimensions, format, mip_level_count)?)
                .ok_or(UiRenderRejectReason::InvalidTextureExtent)?;
            if bytes > MAX_UI_TEXTURE_BYTES {
                return Err(UiRenderRejectReason::TextureByteLimitExceeded {
                    actual: bytes,
                    limit: MAX_UI_TEXTURE_BYTES,
                });
            }
            let bucket = if let Some(index) = buckets.iter().position(|b| {
                b.dimensions == dimensions
                    && b.format == format
                    && b.mip_level_count == mip_level_count
            }) {
                index
            } else {
                if buckets.len() == MAX_UI_TEXTURE_BUCKETS {
                    return Err(UiRenderRejectReason::InvalidTextureExtent);
                }
                buckets.push(UiTextureBucket {
                    dimensions,
                    format,
                    layers: 0,
                    mip_level_count,
                });
                buckets.len() - 1
            };
            locations.push(UiTextureLocation {
                bucket,
                layer: buckets[bucket].layers,
            });
            buckets[bucket].layers += 1;
        }
        Ok(Self {
            buckets: buckets.into(),
            locations: locations.into(),
            bytes,
        })
    }

    pub fn validate_device(
        &self,
        max_side: u32,
        max_layers: u32,
    ) -> Result<(), UiRenderRejectReason> {
        if self
            .buckets
            .iter()
            .any(|b| b.dimensions.iter().any(|&side| side > max_side) || b.layers > max_layers)
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        Ok(())
    }
    pub fn buckets(&self) -> &[UiTextureBucket] {
        &self.buckets
    }
    pub fn locations(&self) -> &[UiTextureLocation] {
        &self.locations
    }
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// The storage this plan needs when the pages `blank` marks hold only zeros: in each
    /// bucket, the blank pages after its last non-blank page share one layer after it, so
    /// unused reserved slots cost one layer per bucket instead of one each. Earlier pages
    /// keep their layers, so filling a slot or emptying one moves no other page.
    fn sharing_blank(&self, blank: impl Fn(usize) -> bool) -> Self {
        let mut used = vec![0_u32; self.buckets.len()];
        for (page, location) in self.locations.iter().enumerate() {
            if !blank(page) {
                used[location.bucket] = used[location.bucket].max(location.layer + 1);
            }
        }
        let mut shared = vec![false; self.buckets.len()];
        let locations: Box<[UiTextureLocation]> = self
            .locations
            .iter()
            .map(|location| {
                let used = used[location.bucket];
                if location.layer < used {
                    *location
                } else {
                    shared[location.bucket] = true;
                    UiTextureLocation {
                        bucket: location.bucket,
                        layer: used,
                    }
                }
            })
            .collect();
        let buckets: Box<[UiTextureBucket]> = self
            .buckets
            .iter()
            .zip(used.iter().zip(&shared))
            .map(|(bucket, (&used, &shared))| UiTextureBucket {
                layers: used + u32::from(shared),
                ..*bucket
            })
            .collect();
        let bytes = buckets.iter().map(|bucket| bucket.byte_len()).sum();
        Self {
            buckets,
            locations,
            bytes,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UiTextureCatalog {
    pages: Arc<[UiTexturePage]>,
    plan: UiTexturePlan,
    /// `plan` with blank trailing pages sharing a layer: what the GPU allocates.
    resident: UiTexturePlan,
    dynamic_start: usize,
    identity: [u8; 32],
    static_identity: [u8; 32],
    source_identity: [u8; 32],
}

impl PartialEq for UiTextureCatalog {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.static_identity == other.static_identity
            && self.plan == other.plan
            && self.dynamic_start == other.dynamic_start
    }
}
impl Eq for UiTextureCatalog {}

impl UiTextureCatalog {
    pub fn new(
        pages: Vec<UiTexturePage>,
        dynamic_start: usize,
    ) -> Result<Self, UiRenderRejectReason> {
        Self::with_source_identity(pages, dynamic_start, [0; 32])
    }

    /// Source identity is an invalidation namespace, never proof of pixels or
    /// budget admission. Production derives it from its loaded asset catalogs.
    pub fn with_source_identity(
        pages: Vec<UiTexturePage>,
        dynamic_start: usize,
        source_identity: [u8; 32],
    ) -> Result<Self, UiRenderRejectReason> {
        if dynamic_start > pages.len()
            || pages.is_empty()
            || pages.len() > MAX_UI_TEXTURE_LAYERS as usize
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let dynamic = &pages[dynamic_start..];
        let art = dynamic
            .iter()
            .enumerate()
            .filter(|(offset, page)| {
                (MAX_UI_DYNAMIC_PAGES..UI_NATIVE_SHEET_PAGE_OFFSET).contains(offset)
                    && page.dimensions == [UI_ART_PAGE_SIDE; 2]
            })
            .count();
        let native = dynamic
            .len()
            .saturating_sub(UI_NATIVE_SHEET_PAGE_OFFSET)
            .min(MAX_UI_NATIVE_SHEET_PAGES);
        let selected = dynamic.len().saturating_sub(UI_SELECTED_MODEL_PAGE_OFFSET);
        let small = dynamic.len() - art - native - selected;
        if small > MAX_UI_DYNAMIC_PAGES
            || art > MAX_UI_ART_PAGES
            || native > MAX_UI_NATIVE_SHEET_PAGES
            || dynamic.iter().enumerate().any(|(offset, page)| {
                (matches!(&page.pixels, Pixels::Font { .. }) && offset != UI_LOCAL_FONT_PAGE_OFFSET)
                    || !valid_dynamic_dimensions(offset, page.dimensions, page.format)
            })
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let blank_bytes = UI_DYNAMIC_PAGE_SIDE as usize * UI_DYNAMIC_PAGE_SIDE as usize * 4;
        let native_bytes = dynamic
            .iter()
            .skip(UI_NATIVE_SHEET_PAGE_OFFSET)
            .map(|page| page.byte_len().saturating_sub(blank_bytes))
            .sum::<usize>();
        if native_bytes > MAX_UI_NATIVE_SHEET_BYTES {
            return Err(UiRenderRejectReason::TextureByteLimitExceeded {
                actual: native_bytes,
                limit: MAX_UI_NATIVE_SHEET_BYTES,
            });
        }
        let planned = pages
            .iter()
            .map(|page| {
                (
                    page.resident_dimensions(),
                    page.format,
                    page.mip_level_count(),
                )
            })
            .collect::<Vec<_>>();
        let plan = UiTexturePlan::with_levels(&planned)?;
        let fixed_bytes = fixed_budget_bytes(&pages, dynamic_start);
        if fixed_bytes > MAX_UI_FIXED_TEXTURE_BYTES {
            return Err(UiRenderRejectReason::TextureByteLimitExceeded {
                actual: fixed_bytes,
                limit: MAX_UI_FIXED_TEXTURE_BYTES,
            });
        }
        let mut all = Sha256::new();
        let mut static_pages = Sha256::new();
        all.update(source_identity);
        static_pages.update(source_identity);
        all.update((dynamic_start as u64).to_le_bytes());
        static_pages.update((dynamic_start as u64).to_le_bytes());
        // Reserved logical slots keep their identity when model or session icon pages resize.
        all.update((pages.len() as u64).to_le_bytes());
        // Selected sources may enter and leave after admission. Their count belongs to the
        // full catalog identity; the fixed identity describes only the retained producers.
        let fixed_pages = pages
            .len()
            .min(dynamic_start + UI_SELECTED_MODEL_PAGE_OFFSET);
        static_pages.update((fixed_pages as u64).to_le_bytes());
        for (index, page) in pages.iter().enumerate() {
            for side in page.dimensions {
                all.update(side.to_le_bytes());
                if index < dynamic_start || !is_resizable_slot(index - dynamic_start) {
                    static_pages.update(side.to_le_bytes());
                }
            }
            all.update(page.identity);
            if index < dynamic_start {
                static_pages.update(page.identity);
            }
        }
        let resident = plan.sharing_blank(|page| pages[page].is_blank());
        Ok(Self {
            pages: pages.into(),
            plan,
            resident,
            dynamic_start,
            identity: all.finalize().into(),
            static_identity: static_pages.finalize().into(),
            source_identity,
        })
    }

    pub fn replace_dynamic(
        &self,
        replacement: Vec<UiTexturePage>,
    ) -> Result<Self, UiRenderRejectReason> {
        let fixed = UI_SELECTED_MODEL_PAGE_OFFSET;
        let old_len = self.pages.len() - self.dynamic_start;
        if (replacement.len() != old_len && (replacement.len() < fixed || old_len < fixed))
            || replacement
                .iter()
                .zip(&self.pages[self.dynamic_start..])
                .enumerate()
                .any(|(offset, (a, b))| !is_resizable_slot(offset) && a.dimensions != b.dimensions)
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let mut pages = self.pages[..self.dynamic_start].to_vec();
        pages.extend(replacement);
        Self::with_source_identity(pages, self.dynamic_start, self.source_identity)
    }
    pub fn pages(&self) -> &[UiTexturePage] {
        &self.pages
    }
    pub fn plan(&self) -> &UiTexturePlan {
        &self.plan
    }
    /// The layout the GPU holds the pages in: [`Self::plan`] with each bucket's trailing
    /// blank pages sharing one layer.
    pub fn resident_plan(&self) -> &UiTexturePlan {
        &self.resident
    }
    /// Native slots are charged at their blank extent; only their growth uses reserved capacity.
    pub fn fixed_budget_bytes(&self) -> usize {
        fixed_budget_bytes(&self.pages, self.dynamic_start)
    }
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
    pub const fn static_identity(&self) -> [u8; 32] {
        self.static_identity
    }
    pub const fn dynamic_start(&self) -> usize {
        self.dynamic_start
    }
}

fn fixed_budget_bytes(pages: &[UiTexturePage], dynamic_start: usize) -> usize {
    let blank = UI_DYNAMIC_PAGE_SIDE as usize
        * UI_DYNAMIC_PAGE_SIDE as usize
        * UiTextureFormat::Rgba8.bytes_per_texel();
    pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            if index >= dynamic_start
                && index - dynamic_start != UI_LOCAL_FONT_PAGE_OFFSET
                && is_resizable_slot(index - dynamic_start)
            {
                blank
            } else {
                page.byte_len()
            }
        })
        .sum()
}

/// Original sources can resize within reserved capacity without changing logical slots.
fn is_resizable_slot(offset: usize) -> bool {
    offset >= UI_SELECTED_MODEL_PAGE_OFFSET
        || offset == UI_LOCAL_FONT_PAGE_OFFSET
        || UI_SESSION_MODEL_PAGES.contains(&offset)
        || offset == UI_PARTICLE_PAGE_OFFSET
        || (UI_PLAYER_SKIN_PAGE_OFFSET..=UI_SESSION_ICON_PAGE_OFFSET).contains(&offset)
        || (UI_SESSION_GLYPH_PAGE_OFFSET..UI_SESSION_GLYPH_PAGE_OFFSET + UI_SESSION_GLYPH_PAGES)
            .contains(&offset)
        || (UI_NATIVE_SHEET_PAGE_OFFSET..UI_NATIVE_SHEET_PAGE_OFFSET + MAX_UI_NATIVE_SHEET_PAGES)
            .contains(&offset)
}

/// Accepts only the dimensions supported by each reserved dynamic page's producer.
fn valid_dynamic_dimensions(
    offset: usize,
    [width, height]: [u32; 2],
    format: UiTextureFormat,
) -> bool {
    if offset >= UI_NATIVE_SHEET_PAGE_OFFSET {
        return width > 0
            && height > 0
            && width <= MAX_UI_TEXTURE_SIDE
            && height <= MAX_UI_TEXTURE_SIDE;
    }
    if offset == UI_LOCAL_FONT_PAGE_OFFSET {
        return [width, height] == [UI_LOCAL_FONT_PAGE_SIDE; 2]
            || (format == UiTextureFormat::Coverage
                && [width, height] == [UI_LOCAL_ICON_FONT_PAGE_SIDE; 2]);
    }
    if [width, height] == [UI_DYNAMIC_PAGE_SIDE; 2] {
        return true;
    }
    if UI_SESSION_MODEL_PAGES.contains(&offset) {
        return [width, height] == [UI_MODEL_ATLAS_SIDE; 2];
    }
    if (UI_SESSION_GLYPH_PAGE_OFFSET..UI_SESSION_GLYPH_PAGE_OFFSET + UI_SESSION_GLYPH_PAGES)
        .contains(&offset)
    {
        return [width, height] == [UI_SESSION_GLYPH_PAGE_SIDE; 2];
    }
    if offset == UI_PLAYER_SKIN_PAGE_OFFSET {
        let classic = render_api::CLASSIC_SKIN_SIDE as u32;
        return (width == classic && height == classic / 2)
            || (width == height
                && width.is_power_of_two()
                && (classic..=render_api::MAX_STANDARD_SKIN_SIDE).contains(&width));
    }
    if (UI_MODEL_ATLAS_PAGE_OFFSET..UI_SESSION_ICON_PAGE_OFFSET).contains(&offset) {
        return [width, height] == [UI_MODEL_ATLAS_SIDE; 2];
    }
    if offset == UI_SESSION_ICON_PAGE_OFFSET || offset == UI_PARTICLE_PAGE_OFFSET {
        return width == height
            && width.is_power_of_two()
            && (UI_DYNAMIC_PAGE_SIDE..=MAX_UI_TEXTURE_SIDE).contains(&width);
    }
    [width, height] == [UI_ART_PAGE_SIDE; 2]
}

#[cfg(test)]
mod budget_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_page_ownership_is_validated_and_changes_content_identity() {
        let page = UiTexturePage::owned([2, 2], Arc::from([255; 16])).unwrap();
        let left = page.clone().with_repeat_content([0, 0, 1, 2]).unwrap();
        let right = page.clone().with_repeat_content([1, 0, 2, 2]).unwrap();
        assert_ne!(left.identity(), right.identity());
        assert_eq!(left.repeat_content(), Some([0, 0, 1, 2]));
        for bounds in [[0, 0, 3, 2], [2, 0, 1, 2], [0, 1, 2, 1]] {
            assert!(page.clone().with_repeat_content(bounds).is_err());
        }
    }

    /// Pack equipment takes whole model-atlas pages, resizing within the native growth budget.
    #[test]
    fn pack_equipment_slots_hold_model_atlas_pages() {
        for offset in UI_PACK_EQUIPMENT_PAGE_OFFSET
            ..UI_PACK_EQUIPMENT_PAGE_OFFSET + MAX_UI_PACK_EQUIPMENT_PAGES
        {
            assert!(valid_dynamic_dimensions(
                offset,
                [UI_MODEL_ATLAS_SIDE; 2],
                UiTextureFormat::Rgba8
            ));
            assert!(is_resizable_slot(offset));
        }
    }

    /// Dense icon coverage fits the old local-font byte budget; RGBA cannot use the larger extent.
    #[test]
    fn dense_local_font_coverage_preserves_the_existing_byte_budget() {
        let dense = [UI_LOCAL_ICON_FONT_PAGE_SIDE; 2];
        assert!(valid_dynamic_dimensions(
            UI_LOCAL_FONT_PAGE_OFFSET,
            dense,
            UiTextureFormat::Coverage
        ));
        assert!(!valid_dynamic_dimensions(
            UI_LOCAL_FONT_PAGE_OFFSET,
            dense,
            UiTextureFormat::Rgba8
        ));
        assert_eq!(
            dense[0] as usize * dense[1] as usize * UiTextureFormat::Coverage.bytes_per_texel(),
            (UI_LOCAL_FONT_PAGE_SIDE * UI_LOCAL_FONT_PAGE_SIDE) as usize
                * UiTextureFormat::Rgba8.bytes_per_texel(),
        );
        assert!(!valid_dynamic_dimensions(
            UI_LOCAL_FONT_PAGE_OFFSET,
            [UI_LOCAL_ICON_FONT_PAGE_SIDE * 2; 2],
            UiTextureFormat::Coverage
        ));
    }

    #[test]
    fn native_fonts_art_and_skin_dimensions_share_the_resident_budget() {
        let mut pages = [256, 512, 1024]
            .map(|side| ([side; 2], UiTextureFormat::Coverage))
            .to_vec();
        pages.extend(
            [16, 256, 512, 1024, 2048, 3072].map(|side| ([side; 2], UiTextureFormat::Rgba8)),
        );
        let plan = UiTexturePlan::with_formats(&pages).unwrap();
        assert_eq!(plan.buckets().len(), pages.len());
        assert!(plan.bytes() < MAX_UI_TEXTURE_BYTES);
        for (page, location) in pages.iter().zip(plan.locations()) {
            let bucket = plan.buckets()[location.bucket];
            assert_eq!((bucket.dimensions, bucket.format), *page);
            assert_eq!(location.layer, 0);
        }
        assert!(plan.validate_device(4096, 1).is_ok());
    }

    /// Coverage pages get their own one-byte-per-texel buckets beside same-sized RGBA pages.
    #[test]
    fn coverage_pages_plan_separate_quarter_size_buckets() {
        let side = [64, 64];
        let plan = UiTexturePlan::with_formats(&[
            (side, UiTextureFormat::Coverage),
            (side, UiTextureFormat::Rgba8),
            (side, UiTextureFormat::Coverage),
        ])
        .unwrap();
        assert_eq!(plan.buckets().len(), 2);
        assert_eq!(plan.buckets()[0].format, UiTextureFormat::Coverage);
        assert_eq!(plan.buckets()[0].layers, 2);
        assert_eq!(
            plan.locations()[2],
            UiTextureLocation {
                bucket: 0,
                layer: 1
            }
        );
        assert_eq!(plan.bytes(), 64 * 64 * (1 + 4 + 1));
    }

    /// Blank pages after a bucket's last used page share one layer; earlier ones keep theirs.
    #[test]
    fn trailing_blank_pages_share_one_resident_layer() {
        let side = UI_DYNAMIC_PAGE_SIDE;
        let page = |value: u8| {
            UiTexturePage::owned([side; 2], vec![value; (side * side * 4) as usize].into()).unwrap()
        };
        let art = || {
            let side = UI_ART_PAGE_SIDE as usize;
            UiTexturePage::owned([UI_ART_PAGE_SIDE; 2], vec![0; side * side * 4].into()).unwrap()
        };
        // Bucket 0: used, blank, used, blank, blank. Bucket 1: blank, blank.
        let pages = [page(1), page(0), page(2), art(), page(0), art(), page(0)];
        let plan = UiTexturePlan::with_formats(
            &pages
                .iter()
                .map(|page| (page.dimensions(), page.format()))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let resident = plan.sharing_blank(|index| pages[index].is_blank());
        let layers: Vec<_> = resident
            .locations()
            .iter()
            .map(|location| (location.bucket, location.layer))
            .collect();
        assert_eq!(
            layers,
            [(0, 0), (0, 1), (0, 2), (1, 0), (0, 3), (1, 0), (0, 3)]
        );
        assert_eq!(
            resident
                .buckets()
                .iter()
                .map(|b| b.layers)
                .collect::<Vec<_>>(),
            [4, 1]
        );
        let rgba = |side: u32| side as usize * side as usize * 4;
        assert_eq!(
            resident.bytes(),
            4 * rgba(side) + rgba(UI_ART_PAGE_SIDE),
            "one of five small layers and one of two art layers saved"
        );
    }
}
