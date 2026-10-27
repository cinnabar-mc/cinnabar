//! Decorated shield rasters retain the selected model's exact base artwork.
use super::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    pattern: render_api::ShieldPattern,
    source: usize,
    layer: u32,
    size: [u16; 2],
}

struct Image {
    /// Keeps the pointer used by the key alive until its cached composition is retired.
    _source: Arc<[u8]>,
    raster: EquipmentRaster,
    used: bool,
}

#[derive(Default)]
pub(super) struct Patterns(BTreeMap<Key, Image>);

impl EquipmentRuntime {
    /// Starts a frame's visible shield set without throwing away unchanged compositions.
    pub(crate) fn begin_shield_frame(&mut self) {
        for image in self.shield_patterns.0.values_mut() {
            image.used = false;
        }
    }

    /// Releases compositions no longer used by a visible hand or actor.
    pub(crate) fn finish_shield_frame(&mut self) {
        self.shield_patterns.0.retain(|_, image| image.used);
    }

    /// Composes the admitted shield stack over the raster selected by its actual draw layer.
    pub(crate) fn shield_raster(
        &mut self,
        item: &WornItem,
        artwork: &ActorArtworkPages,
        location: ActorArtworkLocation,
    ) -> Option<EquipmentRaster> {
        if item.identifier.as_ref() != assets::gui_item::SHIELD_IDENTIFIER {
            return None;
        }
        let pattern = item.shield_pattern.as_ref()?;
        let page = artwork
            .pages()
            .get(usize::from(location.page()).checked_sub(1)?)?;
        if location.layer() >= page.layers() {
            return None;
        }
        let pixels = page.shared_pixels();
        let (width, height) = page.dimensions();
        let key = Key {
            pattern: pattern.as_ref().clone(),
            source: pixels.as_ptr() as usize,
            layer: location.layer(),
            size: [width, height],
        };
        if !self.shield_patterns.0.contains_key(&key) {
            let bytes = usize::from(width) * usize::from(height) * 4;
            let start = usize::try_from(location.layer()).ok()?.checked_mul(bytes)?;
            let base = assets::EquipmentTexture {
                identifier: item.identifier.as_ref().into(),
                width,
                height,
                rgba8: Arc::from(pixels.get(start..start.checked_add(bytes)?)?),
            };
            let startup = self.catalog.as_deref()?;
            let pack = self.pack.as_ref().and_then(|pack| pack.catalog.as_deref());
            let composed = assets::item::shield::compose(
                &base,
                pattern.base,
                pattern
                    .layers
                    .iter()
                    .map(|layer| (layer.pattern.as_ref(), layer.color)),
                usize::from(width) * usize::from(height),
                |path| {
                    pack.and_then(|catalog| catalog.texture(path))
                        .or_else(|| startup.texture(path))
                },
            )?;
            self.shield_patterns.0.insert(
                key.clone(),
                Image {
                    _source: pixels,
                    raster: EquipmentRaster {
                        width: composed.width,
                        height: composed.height,
                        rgba8: composed.rgba8,
                    },
                    used: true,
                },
            );
        }
        let image = self.shield_patterns.0.get_mut(&key)?;
        image.used = true;
        Some(image.raster.clone())
    }
}
