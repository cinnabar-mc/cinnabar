//! Immutable startup artwork pages. Pixel decoding and hashing never run per frame.
use assets::RuntimeActorCatalog;
use sha2::Digest;

use crate::EntityRigId;
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[cfg(test)]
#[path = "artwork/color_mask_tests.rs"]
mod color_mask_tests;
#[path = "artwork/multitexture.rs"]
mod multitexture;
#[path = "artwork/page_budget.rs"]
mod page_budget;
#[path = "artwork/page_pixels.rs"]
mod page_pixels;
pub use page_pixels::PagePixels;
#[cfg(test)]
#[path = "artwork/page_capacity_tests.rs"]
mod page_capacity_tests;
use page_budget::push_group;
#[path = "artwork/pixel_cache.rs"]
mod pixel_cache;
#[cfg(test)]
#[path = "artwork/raster_page_tests.rs"]
mod raster_page_tests;
#[path = "artwork/raster_pages.rs"]
mod raster_pages;
#[cfg(test)]
#[path = "artwork/render_coverage.rs"]
mod render_coverage;
#[cfg(test)]
#[path = "artwork/retained_equipment_tests.rs"]
mod retained_equipment_tests;
#[cfg(test)]
#[path = "artwork/shared_layers_tests.rs"]
mod shared_layers_tests;

/// CPU draw routing selects an artwork page independently of the shader's texture layer.
pub type ActorArtworkPageId = u16;
/// Player skins occupy page zero; generic artwork occupies the remaining page IDs.
pub const MAX_ACTOR_TEXTURE_PAGES: usize = ActorArtworkPageId::MAX as usize + 1;
// Cinnabar declared RGBA allocation ceiling, not retail or measured driver memory: vanilla
// startup art takes about 20 MiB. It covers full pack and equipment art; a page past it is
// downscaled to fit, never dropped.
pub const MAX_ACTOR_GPU_PIXEL_BYTES: usize = 2 * assets::MAX_ACTOR_PIXEL_BYTES;

/// One equipment raster (item sprite or attachable texture) to place on a generic page.
#[derive(Clone, Debug)]
pub struct EquipmentRaster {
    pub width: u16,
    pub height: u16,
    pub rgba8: Arc<[u8]>,
}

impl EquipmentRaster {
    /// Box-filters this raster by the smallest power of two that fits the device limit.
    pub fn fit_within(&self, limit: u32) -> std::borrow::Cow<'_, Self> {
        let Some(([width, height], rgba8)) =
            crate::fit_rgba_within(self.width, self.height, &self.rgba8, limit)
        else {
            return std::borrow::Cow::Borrowed(self);
        };
        std::borrow::Cow::Owned(Self {
            width,
            height,
            rgba8: rgba8.into(),
        })
    }
}

const fn player_page_bytes() -> usize {
    crate::PLAYER_SKIN_BUDGET_BYTES
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorArtworkLocation {
    pub page: ActorArtworkPageId,
    pub layer: u32,
    pub pose_mode: assets::ActorPoseMode,
    pub multitexture: Option<[u32; 2]>,
}
impl ActorArtworkLocation {
    pub fn pose_mode(self) -> assets::ActorPoseMode {
        self.pose_mode
    }
    pub fn page(self) -> ActorArtworkPageId {
        self.page
    }
    pub fn layer(self) -> u32 {
        self.layer
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorTexturePage {
    pub width: u16,
    pub height: u16,
    pub layers: u32,
    pub rgba8: PagePixels,
    /// Native USE_COLOR_MASK rasters cannot share a neutral-opacity material binding.
    pub color_mask: bool,
    pub multitexture: bool,
}
impl ActorTexturePage {
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    pub fn layers(&self) -> u32 {
        self.layers
    }
    /// Every layer's bytes in one slice; copies the whole page when layers keep separate
    /// allocations, so per-texel readers use `rgba8.range` and size checks use `rgba8.len`.
    pub fn pixels(&self) -> std::borrow::Cow<'_, [u8]> {
        self.rgba8.joined()
    }
    pub fn shared_pixels(&self) -> PagePixels {
        self.rgba8.clone()
    }

    /// This page box-filtered down by the smallest power of two that fits `limit` per side.
    pub fn fit_within(&self, limit: u32) -> std::borrow::Cow<'_, Self> {
        let mut fitted: Option<([u16; 2], Vec<u8>)> = None;
        for part in self.rgba8.parts() {
            let Some((size, pixels)) = crate::fit_rgba_within(self.width, self.height, part, limit)
            else {
                return std::borrow::Cow::Borrowed(self);
            };
            match &mut fitted {
                Some((_, joined)) => joined.extend_from_slice(&pixels),
                None => fitted = Some((size, pixels)),
            }
        }
        let Some(([width, height], rgba8)) = fitted else {
            return std::borrow::Cow::Borrowed(self);
        };
        std::borrow::Cow::Owned(Self {
            width,
            height,
            layers: self.layers,
            rgba8: rgba8.into(),
            color_mask: self.color_mask,
            multitexture: self.multitexture,
        })
    }
}

/// An admitted source binding and a shared witness of the page that contains it.
#[derive(Clone, Debug)]
struct RetainedEquipmentArtwork {
    location: ActorArtworkLocation,
    page: ActorTexturePage,
}

#[derive(Clone, Debug, Default)]
pub struct ActorArtworkPages {
    pub identity: [u8; 32],
    pub entity_identity: [u8; 32],
    pub actor_glint: Option<EquipmentRaster>,
    pub item_glint: Option<EquipmentRaster>,
    pub pages: Arc<[ActorTexturePage]>,
    routes: Arc<BTreeMap<EntityRigId, ActorArtworkLocation>>,
    /// Location of every catalog texture by entity-catalog source index.
    source_locations: Arc<BTreeMap<u32, ActorArtworkLocation>>,
    /// `(page, layer)` of every catalog texture; any entity rig may draw these variants.
    entity_locations: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    /// `(page, layer)` of every equipment raster; equipment rigs are not entity routes.
    equipment: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    /// Immutable source identities retain their admitted equipment layer without scanning pixels.
    equipment_artwork: Arc<BTreeMap<[u8; 32], RetainedEquipmentArtwork>>,
    /// Session pack textures by pack-catalog source index, a separate index space.
    pack_source_locations: Arc<BTreeMap<u32, ActorArtworkLocation>>,
    pack_locations: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    rejected_bindings: usize,
}
impl ActorArtworkPages {
    /// Preserves reusable pixels when appending or replacing a collection's page records.
    fn replace_pages(&mut self, pages: Vec<ActorTexturePage>, keys: &[([u8; 32], usize)]) {
        let previous = std::mem::replace(&mut self.pages, pages.into());
        pixel_cache::remember(&previous, &self.pages, keys);
    }

    /// Installs the shared actor glint image once, independently of skin and armor pages.
    #[must_use]
    pub fn with_actor_glint(mut self, raster: EquipmentRaster) -> Self {
        self = self.with_glint_raster(raster, false);
        self
    }

    /// Installs item glint beside the actor raster so both profiles can draw together.
    #[must_use]
    pub fn with_item_glint(self, raster: EquipmentRaster) -> Self {
        self.with_glint_raster(raster, true)
    }

    /// Hashes a validated raster under its sampler role before replacing that role.
    fn with_glint_raster(mut self, raster: EquipmentRaster, item: bool) -> Self {
        if raster.width == 0
            || raster.height == 0
            || raster.rgba8.len() != usize::from(raster.width) * usize::from(raster.height) * 4
        {
            return self;
        }
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        hasher.update([u8::from(item)]);
        hasher.update(raster.width.to_le_bytes());
        hasher.update(raster.height.to_le_bytes());
        hasher.update(&raster.rgba8);
        self.identity = hasher.finalize().into();
        if item {
            self.item_glint = Some(raster);
        } else {
            self.actor_glint = Some(raster);
        }
        self
    }

    pub fn new(catalog: &RuntimeActorCatalog) -> Self {
        let dimensions = multitexture::page_dimensions(catalog);
        let mut groups = BTreeMap::<(u16, u16, bool, bool), Vec<usize>>::new();
        for (index, texture) in catalog.textures().iter().enumerate() {
            let multitexture = catalog.texture_uses_multitexture(index);
            let (width, height) = if multitexture {
                dimensions
            } else {
                (texture.width, texture.height)
            };
            groups
                .entry((
                    width,
                    height,
                    catalog.texture_uses_color_mask(index),
                    multitexture,
                ))
                .or_default()
                .push(index);
        }
        let mut pages = Vec::new();
        let mut locations = BTreeMap::new();
        // The existing player page retains all 128 layers and its full byte budget.
        let mut gpu_bytes = player_page_bytes();
        for ((width, height, color_mask, multitexture), indices) in groups {
            push_group(
                &mut pages,
                &mut gpu_bytes,
                &indices,
                [width, height],
                multitexture,
                |indices| {
                    let page = ActorTexturePage {
                        width,
                        height,
                        layers: indices.len() as u32,
                        rgba8: multitexture::page_pixels(catalog, indices, width, height).into(),
                        color_mask,
                        multitexture,
                    };
                    (page, ())
                },
                |indices, page, (), _| {
                    for (layer, index) in indices.iter().enumerate() {
                        locations.insert(
                            *index as u32,
                            ActorArtworkLocation {
                                page,
                                layer: layer as u32,
                                pose_mode: assets::ActorPoseMode::CompiledLiteral,
                                multitexture: None,
                            },
                        );
                    }
                },
            );
        }
        let routes: BTreeMap<_, _> = catalog
            .bindings()
            .iter()
            .filter_map(|binding| {
                locations
                    .get(&binding.texture)
                    .copied()
                    .map(|mut location| {
                        location.pose_mode = binding.pose_mode;
                        (EntityRigId(binding.geometry_candidate), location)
                    })
            })
            .collect();
        let rejected_bindings = catalog.bindings().len() - routes.len();
        let source_locations: BTreeMap<u32, ActorArtworkLocation> = catalog
            .textures()
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                Some((texture.source, locations.get(&(index as u32)).copied()?))
            })
            .collect();
        let entity_locations: BTreeSet<(ActorArtworkPageId, u32)> = source_locations
            .values()
            .map(|location| (location.page, location.layer))
            .collect();
        Self {
            source_locations: Arc::new(source_locations),
            entity_locations: Arc::new(entity_locations),
            identity: catalog.identity(),
            entity_identity: catalog.entity_identity(),
            actor_glint: None,
            item_glint: None,
            pages: pages.into(),
            routes: Arc::new(routes),
            equipment: Arc::new(BTreeSet::new()),
            equipment_artwork: Arc::default(),
            pack_source_locations: Arc::default(),
            pack_locations: Arc::default(),
            rejected_bindings,
        }
    }

    /// Retains an admitted equipment layer under its immutable source identity.
    /// The identity must cover source dimensions, pixels, placements and material interpretation.
    pub fn register_equipment_artwork(
        &mut self,
        identity: [u8; 32],
        location: ActorArtworkLocation,
    ) {
        if !self.equipment.contains(&(location.page, location.layer)) {
            return;
        }
        let Some(page) = location
            .page
            .checked_sub(1)
            .and_then(|page| self.pages.get(usize::from(page)))
        else {
            return;
        };
        if location.layer >= page.layers {
            return;
        }
        Arc::make_mut(&mut self.equipment_artwork).insert(
            identity,
            RetainedEquipmentArtwork {
                location,
                page: page.clone(),
            },
        );
    }

    /// Reuses a retained source while its admitted page still shares the same pixels and layout.
    /// Appending pages preserves bindings; replacing public page data invalidates the witness.
    pub fn equipment_artwork_location(&self, identity: [u8; 32]) -> Option<ActorArtworkLocation> {
        let retained = self.equipment_artwork.get(&identity)?;
        let page = self
            .pages
            .get(usize::from(retained.location.page.checked_sub(1)?))?;
        (page.rgba8.ptr_eq(&retained.page.rgba8)
            && page.width == retained.page.width
            && page.height == retained.page.height
            && page.layers == retained.page.layers
            && page.color_mask == retained.page.color_mask
            && page.multitexture == retained.page.multitexture
            && self
                .equipment
                .contains(&(retained.location.page, retained.location.layer)))
        .then_some(retained.location)
    }

    /// Appends pages for `rasters`, grouped by size, and returns each raster's location (`None`
    /// when its group would exceed the page or byte budget). The identity changes to cover them.
    #[must_use]
    pub fn with_equipment_rasters(
        self,
        rasters: &[EquipmentRaster],
    ) -> (Self, Vec<Option<ActorArtworkLocation>>) {
        self.with_raster_materials(rasters, &[])
    }

    fn with_raster_materials(
        mut self,
        rasters: &[EquipmentRaster],
        color_masks: &[bool],
    ) -> (Self, Vec<Option<ActorArtworkLocation>>) {
        let mut groups = BTreeMap::<(u16, u16, bool), Vec<usize>>::new();
        for (index, raster) in rasters.iter().enumerate() {
            if raster.width != 0
                && raster.height != 0
                && raster.rgba8.len() == usize::from(raster.width) * usize::from(raster.height) * 4
            {
                groups
                    .entry((
                        raster.width,
                        raster.height,
                        color_masks.get(index).copied().unwrap_or(false),
                    ))
                    .or_default()
                    .push(index);
            }
        }
        let mut pages = self.pages.to_vec();
        let mut shared = Vec::new();
        let mut gpu_bytes = pages
            .iter()
            .fold(player_page_bytes(), |total, page| total + page.rgba8.len());
        let mut locations = vec![None; rasters.len()];
        let mut equipment = (*self.equipment).clone();
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        for ((width, height, color_mask), indices) in groups {
            push_group(
                &mut pages,
                &mut gpu_bytes,
                &indices,
                [width, height],
                false,
                |indices| {
                    hasher.update(width.to_le_bytes());
                    hasher.update(height.to_le_bytes());
                    hasher.update((indices.len() as u32).to_le_bytes());
                    let bytes: usize = indices
                        .iter()
                        .map(|index| rasters[*index].rgba8.len())
                        .sum();
                    hasher.update((bytes as u64).to_le_bytes());
                    hasher.update([u8::from(color_mask)]);
                    hasher.update([0u8]); // Equipment/overrides are never three-sampler pages.
                    let (key, pixels) = pixel_cache::pack(
                        indices.iter().map(|index| &rasters[*index].rgba8),
                        &mut hasher,
                    );
                    let page = ActorTexturePage {
                        width,
                        height,
                        layers: indices.len() as u32,
                        rgba8: pixels,
                        color_mask,
                        multitexture: false,
                    };
                    (page, key)
                },
                |indices, page, key, kept| {
                    if kept {
                        shared.push((key, usize::from(page) - 1));
                    }
                    for (layer, index) in indices.iter().enumerate() {
                        locations[*index] = Some(ActorArtworkLocation {
                            page,
                            layer: layer as u32,
                            pose_mode: assets::ActorPoseMode::CompiledLiteral,
                            multitexture: None,
                        });
                        equipment.insert((page, layer as u32));
                    }
                },
            );
        }
        if pages.len() != self.pages.len() {
            self.identity = hasher.finalize().into();
            self.replace_pages(pages, &shared);
            self.equipment = Arc::new(equipment);
        }
        (self, locations)
    }
    /// Appends session artwork under pack rig IDs, replacing its previous variant table.
    /// Call on startup pages so removed packs release their routes and pixel budget.
    #[must_use]
    pub fn with_pack_artwork(
        mut self,
        textures: &[assets::ActorTexture],
        bindings: &[assets::ActorArtworkBinding],
    ) -> Self {
        let mut groups = BTreeMap::<(u16, u16), Vec<usize>>::new();
        for (index, texture) in textures.iter().enumerate() {
            groups
                .entry((texture.width, texture.height))
                .or_default()
                .push(index);
        }
        let mut pages = self.pages.to_vec();
        let mut shared = Vec::new();
        let mut gpu_bytes = pages
            .iter()
            .fold(player_page_bytes(), |total, page| total + page.rgba8.len());
        let mut locations = BTreeMap::new();
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        for ((width, height), indices) in groups {
            push_group(
                &mut pages,
                &mut gpu_bytes,
                &indices,
                [width, height],
                false,
                |indices| {
                    hasher.update(width.to_le_bytes());
                    hasher.update(height.to_le_bytes());
                    hasher.update((indices.len() as u32).to_le_bytes());
                    let bytes: usize = indices
                        .iter()
                        .map(|index| textures[*index].rgba8.len())
                        .sum();
                    hasher.update((bytes as u64).to_le_bytes());
                    hasher.update([0u8]); // Pack pages use literal RGBA, like unmasked rasters.
                    hasher.update([0u8]); // No native three-sampler contract for arbitrary packs.
                    let (key, pixels) = pixel_cache::pack(
                        indices.iter().map(|index| &textures[*index].rgba8),
                        &mut hasher,
                    );
                    let page = ActorTexturePage {
                        width,
                        height,
                        layers: indices.len() as u32,
                        rgba8: pixels,
                        color_mask: false,
                        multitexture: false,
                    };
                    (page, key)
                },
                |indices, page, key, kept| {
                    if kept {
                        shared.push((key, usize::from(page) - 1));
                    }
                    for (layer, index) in indices.iter().enumerate() {
                        locations.insert(
                            *index as u32,
                            ActorArtworkLocation {
                                page,
                                layer: layer as u32,
                                pose_mode: assets::ActorPoseMode::CompiledLiteral,
                                multitexture: None,
                            },
                        );
                    }
                },
            );
        }
        let pack_source_locations: BTreeMap<u32, ActorArtworkLocation> = textures
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                Some((texture.source, locations.get(&(index as u32)).copied()?))
            })
            .collect();
        self.pack_locations = Arc::new(
            pack_source_locations
                .values()
                .map(|location| (location.page, location.layer))
                .collect(),
        );
        self.pack_source_locations = Arc::new(pack_source_locations);
        let mut routes = (*self.routes).clone();
        let mut accepted = 0;
        for binding in bindings {
            let Some(mut location) = locations.get(&binding.texture).copied() else {
                continue;
            };
            location.pose_mode = binding.pose_mode;
            routes.insert(crate::pack_rig_id(binding.geometry_candidate), location);
            accepted += 1;
        }
        self.rejected_bindings += bindings.len() - accepted;
        if pages.len() != self.pages.len() {
            self.identity = hasher.finalize().into();
            self.replace_pages(pages, &shared);
        }
        self.routes = Arc::new(routes);
        self
    }

    /// Replaces base texture routes while retaining the new images' original resolution.
    pub fn with_source_texture_overrides(self, overrides: &[(u32, EquipmentRaster)]) -> Self {
        let overrides: Vec<_> = overrides
            .iter()
            .filter(|(source, _)| self.source_locations.contains_key(source))
            .collect();
        let rasters: Vec<_> = overrides.iter().map(|(_, raster)| raster.clone()).collect();
        let color_masks: Vec<_> = overrides
            .iter()
            .map(|(source, _)| {
                let location = self.source_locations[source];
                self.pages[usize::from(location.page) - 1].color_mask
            })
            .collect();
        let (mut pages, locations) = self.with_raster_materials(&rasters, &color_masks);
        let mut sources = (*pages.source_locations).clone();
        let mut routes = (*pages.routes).clone();
        let mut variants = (*pages.entity_locations).clone();
        for ((source, _), replacement) in overrides.into_iter().zip(locations) {
            let (Some(old), Some(mut new)) = (sources.get(source).copied(), replacement) else {
                continue;
            };
            for route in routes.values_mut() {
                if (route.page, route.layer) == (old.page, old.layer) {
                    new.pose_mode = route.pose_mode;
                    *route = new;
                }
            }
            sources.insert(*source, new);
            variants.insert((new.page, new.layer));
        }
        pages.source_locations = Arc::new(sources);
        pages.routes = Arc::new(routes);
        pages.entity_locations = Arc::new(variants);
        pages
    }

    pub fn route(&self, rig: EntityRigId) -> Option<ActorArtworkLocation> {
        self.routes.get(&rig).copied()
    }
    /// Where the catalog texture drawn from entity-catalog source `source` lives, for a body of
    /// rig `rig`; `None` when the rig has no artwork or the source was not built.
    pub fn variant_location(&self, rig: EntityRigId, source: u32) -> Option<ActorArtworkLocation> {
        let route = self.route(rig)?;
        let sources = if crate::is_pack_rig_id(rig) {
            &self.pack_source_locations
        } else {
            &self.source_locations
        };
        let mut location = sources.get(&source).copied()?;
        location.pose_mode = route.pose_mode;
        Some(location)
    }
    /// Where catalog source `source` lives for a body without its own artwork route, such as a
    /// player wearing a runtime skin whose controller also names a catalog texture. `pack`
    /// selects the session pack catalog. The location poses with the body's animated bones.
    pub fn source_location(&self, pack: bool, source: u32) -> Option<ActorArtworkLocation> {
        let sources = if pack {
            &self.pack_source_locations
        } else {
            &self.source_locations
        };
        let mut location = sources.get(&source).copied()?;
        location.pose_mode = assets::ActorPoseMode::CompiledLiteral;
        Some(location)
    }
    pub fn rejected_bindings(&self) -> usize {
        self.rejected_bindings
    }
    pub fn identity(&self) -> [u8; 32] {
        self.identity
    }
    pub fn actor_glint(&self) -> Option<&EquipmentRaster> {
        self.actor_glint.as_ref()
    }

    /// Recognizes clones of the exact artwork snapshot without scanning pixels or routes.
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.entity_identity == other.entity_identity
            && self.rejected_bindings == other.rejected_bindings
            && Arc::ptr_eq(&self.pages, &other.pages)
            && Arc::ptr_eq(&self.routes, &other.routes)
            && Arc::ptr_eq(&self.source_locations, &other.source_locations)
            && Arc::ptr_eq(&self.entity_locations, &other.entity_locations)
            && Arc::ptr_eq(&self.equipment, &other.equipment)
            && Arc::ptr_eq(&self.equipment_artwork, &other.equipment_artwork)
            && Arc::ptr_eq(&self.pack_source_locations, &other.pack_source_locations)
            && Arc::ptr_eq(&self.pack_locations, &other.pack_locations)
    }

    pub fn pages(&self) -> &[ActorTexturePage] {
        &self.pages
    }
    pub fn valid(&self, rig: EntityRigId, location: ActorArtworkLocation) -> bool {
        if !self.valid_multitexture(location) {
            return false;
        }
        // A skin's own model wears the runtime skin, and its animated layers' images live in
        // equipment pages; its controllers may also name any entity texture of either catalog,
        // as a vanilla player's do.
        if crate::is_skin_rig_id(rig) {
            let texture = (location.page, location.layer);
            return self.equipment.contains(&texture)
                || self.pack_locations.contains(&texture)
                || self.entity_locations.contains(&texture);
        }
        if crate::is_equipment_rig_id(rig) {
            return self.equipment.contains(&(location.page, location.layer));
        }
        let variants = if crate::is_pack_rig_id(rig) {
            &self.pack_locations
        } else {
            &self.entity_locations
        };
        // A controller's own geometry draws any entity texture of its catalog.
        if crate::is_layer_geometry_rig_id(rig) {
            return variants.contains(&(location.page, location.layer));
        }
        match self.route(rig) {
            Some(route) => {
                route == location
                    || (route.pose_mode == location.pose_mode
                        && variants.contains(&(location.page, location.layer)))
            }
            None => false,
        }
    }
}

#[cfg(test)]
#[path = "artwork/tests.rs"]
mod tests;
