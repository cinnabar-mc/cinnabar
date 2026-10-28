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
    rig: Option<EntityRigId>,
    used: bool,
}

#[derive(Default)]
pub(super) struct Patterns {
    images: BTreeMap<Key, Image>,
    sprite: Option<EntityRigId>,
}

impl EquipmentRuntime {
    /// Starts a frame's visible shield set without throwing away unchanged compositions.
    pub(crate) fn begin_shield_frame(&mut self) {
        for image in self.shield_patterns.images.values_mut() {
            image.used = false;
        }
    }

    /// Releases compositions no longer used by a visible hand or actor.
    pub(crate) fn finish_shield_frame(&mut self) {
        self.shield_patterns.images.retain(|_, image| image.used);
    }

    /// Releases shield artwork while retaining its pattern-independent sprite geometry.
    pub(super) fn clear_shield_patterns(&mut self) {
        self.begin_shield_frame();
        self.finish_shield_frame();
    }

    /// Returns the selected shield's decorated raster and matching authored or sprite rig.
    pub(crate) fn shield_raster(
        &mut self,
        item: &WornItem,
        artwork: &ActorArtworkPages,
        location: ActorArtworkLocation,
        rig: EntityRigId,
    ) -> Option<(EquipmentRaster, EntityRigId)> {
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
        if !self.shield_patterns.images.contains_key(&key) {
            let bytes = usize::from(width) * usize::from(height) * 4;
            let start = usize::try_from(location.layer()).ok()?.checked_mul(bytes)?;
            let selected = pixels.get(start..start.checked_add(bytes)?)?;
            let (raster, rig) = if icon.is_some() {
                let sprite =
                    self.decorated_shield_icon(pattern, (assets::MAX_ICON_SIDE as usize).pow(2))?;
                let rig = if let Some(rig) = self.shield_patterns.sprite {
                    rig
                } else {
                    // Composed sheets are opaque, so every pattern has the same static outline.
                    let vertices = held_sprite_vertices(
                        usize::from(sprite.width),
                        usize::from(sprite.height),
                        &sprite.rgba8,
                        [0.0, 0.0, 1.0, 1.0],
                    )?;
                    let rig = ReservedItemMesh::ShieldSprite.rig();
                    self.pending
                        .push(ActorRigGeometry::new(rig, vertices, vec![[0.0; 3]]).ok()?);
                    self.shield_patterns.sprite = Some(rig);
                    rig
                };
                (
                    EquipmentRaster {
                        width: sprite.width,
                        height: sprite.height,
                        rgba8: sprite.rgba8,
                    },
                    Some(rig),
                )
            } else {
                let base = assets::EquipmentTexture {
                    identifier: item.identifier.as_ref().into(),
                    width,
                    height,
                    rgba8: Arc::from(selected),
                };
                let composed =
                    self.compose_shield(&base, pattern, usize::from(width) * usize::from(height))?;
                (
                    EquipmentRaster {
                        width: composed.width,
                        height: composed.height,
                        rgba8: composed.rgba8,
                    },
                    None,
                )
            };
            self.shield_patterns.images.insert(
                key.clone(),
                Image {
                    _source: pixels,
                    raster,
                    rig,
                    used: true,
                },
            );
        }
        let image = self.shield_patterns.images.get_mut(&key)?;
        image.used = true;
        Some((image.raster.clone(), image.rig.unwrap_or(rig)))
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
