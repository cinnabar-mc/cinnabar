//! Visible trim artwork is composed once and packed together before icon consumers run.
use super::*;
use std::sync::Mutex;
#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    trim: protocol::ItemTrim,
    binding: usize,
    slot: u8,
    armor_material: Option<assets::facts::ArmourMaterial>,
    source: usize,
    size: [u16; 2],
    dye: Option<u32>,
    icon: bool,
}

struct Image {
    _source: Arc<[u8]>,
    texture: assets::EquipmentTexture,
    region: Option<IconRef>,
    used: bool,
}

#[derive(Default)]
struct Cache {
    cells: BTreeMap<(usize, [u16; 4]), SourceCell>,
    images: BTreeMap<Key, Image>,
    pages: Vec<UiTexturePage>,
    dirty: bool,
}

struct SourceCell {
    _page: UiTexturePage,
    pixels: Arc<[u8]>,
    used: bool,
}

#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct TrimIcons {
    cache: Mutex<Cache>,
}

impl TrimIcons {
    /// Retires unused stack appearances without doing pixel work for unchanged frames.
    pub(in crate::ui_runtime::presentation) fn begin_frame(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        for image in cache.images.values_mut() {
            image.used = false;
        }
        for cell in cache.cells.values_mut() {
            cell.used = false;
        }
    }
    pub(in crate::ui_runtime::presentation) fn clear(&mut self) {
        *self = Self::default();
    }
    pub(in crate::ui_runtime::presentation) fn pages(&self) -> Vec<UiTexturePage> {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pages
            .clone()
    }
    /// Resolves a composed worn sheet by its retained allocation, avoiding repeated pixel hashing.
    pub(in crate::ui_runtime::presentation) fn region(
        &self,
        texture: &player_preview::PreviewTexture,
    ) -> Option<IconRef> {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .images
            .values()
            .find(|image| Arc::ptr_eq(&image.texture.rgba8, &texture.rgba))
            .and_then(|image| image.region)
    }
}

impl UiPresentationRuntime {
    /// Resolves a stack's material-colored inventory trim over the current item artwork.
    pub(in crate::ui_runtime::presentation) fn trim_icon(
        &self,
        identifier: &str,
        metadata: u32,
        trim: &protocol::ItemTrim,
        dye: Option<u32>,
    ) -> Option<IconRef> {
        let base_icon = dye
            .and_then(|rgb| self.color_mask_icon(identifier, metadata, rgb))
            .or_else(|| self.item_icon(identifier, metadata))?;
        // Source pixels come from the selected atlas cell, including session overrides and raw dye masks.
        let page = self.textures.pages().get(usize::from(base_icon.page))?;
        let [pw, ph] = page.dimensions();
        let [x, y, right, bottom] = base_icon.uv;
        let (width, height) = (right.checked_sub(x)?, bottom.checked_sub(y)?);
        if u32::from(right) > pw || u32::from(bottom) > ph {
            return None;
        }
        let source = {
            let mut cache = self
                .gui_models
                .trim_icons
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let key = (page.pixels().as_ptr() as usize, base_icon.uv);
            let cell = cache.cells.entry(key).or_insert_with(|| {
                // Admitted mask and session art are immutable; retain one extracted cell per appearance.
                let mut pixels = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
                for row in y..bottom {
                    let at = (usize::from(row) * pw as usize + usize::from(x)) * 4;
                    pixels.extend_from_slice(&page.pixels()[at..at + usize::from(width) * 4]);
                }
                SourceCell {
                    _page: page.clone(),
                    pixels: pixels.into(),
                    used: true,
                }
            });
            cell.used = true;
            Arc::clone(&cell.pixels)
        };
        let startup = self.equipment_catalog.as_deref()?;
        let binding = startup.binding(identifier)?;
        let trim_binding = self
            .gui_models
            .pack_equipment
            .source
            .as_deref()
            .and_then(|catalog| catalog.binding(identifier))
            .or(Some(binding));
        let base = assets::EquipmentTexture {
            identifier: binding.texture.identifier.clone(),
            width,
            height,
            rgba8: source,
        };
        self.trim_image(
            &base,
            trim_binding,
            trim,
            identifier,
            true,
            base_icon
                .color_mask
                .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])),
        )?
        .1
    }

    /// Shares the decorated sheet with CPU previews and the GPU model atlas.
    pub(in crate::ui_runtime::presentation) fn trim_preview(
        &self,
        preview: &player_preview::PreviewTexture,
        path: &str,
        binding: Option<&assets::EquipmentBinding>,
        trim: &protocol::ItemTrim,
        identifier: &str,
    ) -> Option<player_preview::PreviewTexture> {
        let base = assets::EquipmentTexture {
            identifier: path.into(),
            width: preview.width,
            height: preview.height,
            rgba8: Arc::clone(&preview.rgba),
        };
        let dye = preview
            .tint
            .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]));
        let (texture, _) = self.trim_image(&base, binding, trim, identifier, false, dye)?;
        Some(player_preview::PreviewTexture {
            rgba: texture.rgba8,
            width: texture.width,
            height: texture.height,
            tint: None,
        })
    }

    /// Composes or reuses one stack appearance and marks it as used this frame.
    fn trim_image(
        &self,
        base: &assets::EquipmentTexture,
        binding: Option<&assets::EquipmentBinding>,
        trim: &protocol::ItemTrim,
        identifier: &str,
        icon: bool,
        dye: Option<u32>,
    ) -> Option<(assets::EquipmentTexture, Option<IconRef>)> {
        let startup = self.equipment_catalog.as_deref()?;
        let pack = self.gui_models.pack_equipment.source.as_deref();
        let slot = assets::item::trim::armor_slot(identifier)?;
        let armor_material = assets::facts::ItemFacts::builtin()
            .ok()?
            .by_identifier(identifier)?
            .armor_material();
        let key = Key {
            trim: trim.clone(),
            binding: binding.map_or(0, |binding| binding as *const _ as usize),
            slot: slot as u8,
            armor_material,
            source: base.rgba8.as_ptr() as usize,
            size: [base.width, base.height],
            dye,
            icon,
        };
        let mut cache = self
            .gui_models
            .trim_icons
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !cache.images.contains_key(&key) {
            let texture = assets::item::trim::compose(
                base,
                armor_material,
                &trim.material,
                &trim.pattern,
                slot,
                icon,
                dye.map(|rgb| {
                    let [_, r, g, b] = rgb.to_be_bytes();
                    assets::item::trim::Dye {
                        rgb: [r, g, b],
                        alpha: assets::item::trim::DyeAlpha::Coverage,
                    }
                }),
                binding.map(|binding| &binding.resources),
                |path| {
                    pack.and_then(|p| p.texture(path))
                        .or_else(|| startup.texture(path))
                },
            )?;
            cache.images.insert(
                key.clone(),
                Image {
                    _source: Arc::clone(&base.rgba8),
                    texture,
                    region: None,
                    used: true,
                },
            );
            cache.dirty = true;
        }
        let image = cache.images.get_mut(&key)?;
        image.used = true;
        Some((image.texture.clone(), image.region))
    }

    /// Packs the frame's demanded icons and armor together, reducing resolution only when needed.
    pub(in crate::ui_runtime::presentation) fn finish_trim_art(&mut self) {
        let changed = {
            let mut cache = self
                .gui_models
                .trim_icons
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // Cells of rebuilt pages are no longer referenced, so their retained pages are released here.
            cache.cells.retain(|_, cell| cell.used);
            let count = cache.images.len();
            cache.images.retain(|_, image| image.used);
            if count != cache.images.len() {
                cache.dirty = true;
            }
            if !cache.dirty {
                return;
            }
            let first = (self.textures.dynamic_start() + render_model::UI_TRIM_PAGE_OFFSET) as u16;
            let mut limit = render_model::UI_MODEL_ATLAS_SIDE;
            loop {
                let mut atlas = atlas::Atlas::new(first, render_model::MAX_UI_TRIM_PAGES);
                let regions: Option<Vec<_>> = cache
                    .images
                    .values()
                    .map(|image| {
                        let texture = &image.texture;
                        if let Some((size, pixels)) = render_model::fit_rgba_within(
                            texture.width,
                            texture.height,
                            &texture.rgba8,
                            limit,
                        ) {
                            atlas.insert(size, &pixels).ok()
                        } else {
                            atlas
                                .insert([texture.width, texture.height], &texture.rgba8)
                                .ok()
                        }
                    })
                    .collect();
                if let Some(regions) = regions {
                    for (image, region) in cache.images.values_mut().zip(regions) {
                        image.region = Some(region);
                    }
                    cache.pages = atlas.pages().unwrap_or_default();
                    break;
                }
                if limit == 1 {
                    for image in cache.images.values_mut() {
                        image.region = None;
                    }
                    cache.pages.clear();
                    break;
                }
                limit = (limit / 2).max(1);
            }
            cache.dirty = false;
            true
        };
        if changed {
            if let Some(cache) = self.inventory_cache.as_mut() {
                cache.invalidate_icons();
            }
            self.rebuild_dynamic_textures();
        }
    }
}
