//! Decorated shield rasters retain the selected model's exact base artwork.
use super::*;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    pattern: render_api::ShieldPattern,
    source: usize,
    layer: u32,
    size: [u16; 2],
    icon: Option<[u16; 4]>,
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
        rig: EntityRigId,
    ) -> Option<EquipmentRaster> {
        if item.identifier.as_ref() != assets::gui_item::SHIELD_IDENTIFIER {
            return None;
        }
        let pattern = item.shield_pattern.as_ref()?;
        // Mesh ownership distinguishes a sprite atlas from an attachable UV sheet.
        let icon = match self.meshes.iter().find(|(_, cached)| **cached == Some(rig)) {
            Some((MeshKey::Sprite(index), _)) => Some(*self.placements.get(*index)?.as_ref()?),
            Some((MeshKey::Session(index), _)) => {
                let (selected, placement, selected_location) =
                    self.session_sprite(&item.identifier, item.metadata)?;
                if selected != *index || selected_location != location {
                    return None;
                }
                Some(placement)
            }
            Some(_) => return None,
            None => None,
        };
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
            icon: icon.map(|icon| [icon.x, icon.y, icon.width, icon.height]),
        };
        if !self.shield_patterns.0.contains_key(&key) {
            let bytes = usize::from(width) * usize::from(height) * 4;
            let start = usize::try_from(location.layer()).ok()?.checked_mul(bytes)?;
            let selected = pixels.get(start..start.checked_add(bytes)?)?;
            let raster = if let Some(icon) = icon {
                let sprite = self.decorated_shield_icon(
                    pattern,
                    usize::from(icon.width) * usize::from(icon.height),
                )?;
                let mut rgba8 = selected.to_vec();
                for y in 0..usize::from(icon.height) {
                    for x in 0..usize::from(icon.width) {
                        let source = (y * usize::from(sprite.height) / usize::from(icon.height)
                            * usize::from(sprite.width)
                            + x * usize::from(sprite.width) / usize::from(icon.width))
                            * 4;
                        let target = ((usize::from(icon.y) + y) * usize::from(width)
                            + usize::from(icon.x)
                            + x)
                            * 4;
                        rgba8
                            .get_mut(target..target + 4)?
                            .copy_from_slice(&sprite.rgba8[source..source + 4]);
                    }
                }
                EquipmentRaster {
                    width,
                    height,
                    rgba8: rgba8.into(),
                }
            } else {
                let base = assets::EquipmentTexture {
                    identifier: item.identifier.as_ref().into(),
                    width,
                    height,
                    rgba8: Arc::from(selected),
                };
                let composed =
                    self.compose_shield(&base, pattern, usize::from(width) * usize::from(height))?;
                EquipmentRaster {
                    width: composed.width,
                    height: composed.height,
                    rgba8: composed.rgba8,
                }
            };
            self.shield_patterns.0.insert(
                key.clone(),
                Image {
                    _source: pixels,
                    raster,
                    used: true,
                },
            );
        }
        let image = self.shield_patterns.0.get_mut(&key)?;
        image.used = true;
        Some(image.raster.clone())
    }

    /// Bakes the ordinary shield GUI model before placing it in a held sprite's atlas cell.
    fn decorated_shield_icon(
        &self,
        pattern: &render_api::ShieldPattern,
        max_pixels: usize,
    ) -> Option<IconSprite> {
        let startup = self.catalog.as_deref()?;
        let binding = startup
            .bindings_for(assets::gui_item::SHIELD_IDENTIFIER)
            .iter()
            .rev()
            .find(|binding| {
                binding.attachable.is_none()
                    && self
                        .assets
                        .attachable_rig_binding(binding.attachable_identifier())
                        .is_none_or(|index| {
                            self.assets.rig_bindings()[index].item_condition.is_none()
                        })
            })?;
        let geometry = self
            .assets
            .geometries()
            .iter()
            .find(|geometry| geometry.identifier == binding.geometry.identifier)?;
        let pack = self.pack.as_ref().and_then(|pack| pack.catalog.as_deref());
        let base = pack
            .and_then(|catalog| catalog.texture(&binding.texture.identifier))
            .or_else(|| startup.texture(&binding.texture.identifier))?;
        let composed = self.compose_shield(base, pattern, max_pixels)?;
        assets::item::shield::bake_icon(geometry, &composed)
    }

    /// Resolves the active pack's masks over the selected sheet within the consumer's budget.
    fn compose_shield(
        &self,
        base: &assets::EquipmentTexture,
        pattern: &render_api::ShieldPattern,
        max_pixels: usize,
    ) -> Option<assets::EquipmentTexture> {
        let startup = self.catalog.as_deref()?;
        let pack = self.pack.as_ref().and_then(|pack| pack.catalog.as_deref());
        assets::item::shield::compose(
            base,
            pattern.base,
            pattern
                .layers
                .iter()
                .map(|layer| (layer.pattern.as_ref(), layer.color)),
            max_pixels,
            |path| {
                pack.and_then(|catalog| catalog.texture(path))
                    .or_else(|| startup.texture(path))
            },
        )
    }
}
