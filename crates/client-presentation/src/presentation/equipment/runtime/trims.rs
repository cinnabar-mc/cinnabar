//! Visible armor compositions share source pixels and retire with the frame's equipment.
use super::*;
use assets::item::trim::{Dye, DyeAlpha};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    trim: protocol::ItemTrim,
    item: Arc<str>,
    binding: usize,
    texture: usize,
    source: usize,
    layer: u32,
    dye: Option<Dye>,
    icon: Option<[u16; 4]>,
}

struct Image {
    _source: render_model::PagePixels,
    _binding: Arc<RuntimeEquipmentCatalog>,
    _artwork: Option<Arc<RuntimeEquipmentCatalog>>,
    raster: EquipmentRaster,
    used: bool,
}

/// The admitted definition and actual texture selected for one worn draw.
#[derive(Clone)]
pub(super) struct WornSource {
    pub catalog: Arc<RuntimeEquipmentCatalog>,
    pub binding: usize,
    pub artwork: Arc<RuntimeEquipmentCatalog>,
    pub texture: usize,
}

struct WornDraw {
    identity: render_model::ActorRenderIdentity,
    source: WornSource,
    used: bool,
}

#[derive(Default)]
pub(super) struct Trims {
    images: BTreeMap<Key, Image>,
    worn: BTreeMap<(actor_animation::ActorLifetimeId, u8), WornDraw>,
}

impl Trims {
    /// Reuses the stable actor slot while pinning this frame's exact selection.
    pub(super) fn remember_worn(
        &mut self,
        identity: render_model::ActorRenderIdentity,
        source: WornSource,
    ) {
        self.worn.insert(
            worn_key(identity),
            WornDraw {
                identity,
                source,
                used: true,
            },
        );
    }

    /// Only this frame's admitted definition may decorate a draw with that identity.
    pub(super) fn worn_source(
        &self,
        identity: render_model::ActorRenderIdentity,
    ) -> Option<&WornSource> {
        let draw = self.worn.get(&worn_key(identity))?;
        (draw.used && draw.identity == identity).then_some(&draw.source)
    }

    pub(super) fn begin_frame(&mut self) {
        for draw in self.worn.values_mut() {
            draw.used = false;
        }
        for image in self.images.values_mut() {
            image.used = false;
        }
    }
    pub(super) fn finish_frame(&mut self) {
        self.images.retain(|_, image| image.used);
        self.worn.retain(|_, draw| draw.used);
    }
    pub(super) fn clear(&mut self) {
        self.images.clear();
        self.worn.clear();
    }
}

/// Selection slots follow actor lifetime and layer rather than changing pose generations.
fn worn_key(identity: render_model::ActorRenderIdentity) -> (actor_animation::ActorLifetimeId, u8) {
    (
        actor_animation::ActorLifetimeId {
            session_id: identity.session_id,
            dimension: identity.dimension,
            runtime_id: identity.runtime_id,
            spawn_revision: identity.spawn_revision.get(),
        },
        identity.layer,
    )
}

impl EquipmentRuntime {
    /// Bakes mask RGB below the trim while retaining authored color and opacity on the draw.
    pub(crate) fn trim_draw(
        &mut self,
        item: &WornItem,
        artwork: &ActorArtworkPages,
        location: ActorArtworkLocation,
        draw: &mut render_model::ActorRigSubmission,
        worn: bool,
    ) -> Option<(EquipmentRaster, EntityRigId)> {
        let page = artwork
            .pages()
            .get(usize::from(location.page()).checked_sub(1)?)?;
        let color_mask = page.color_mask
            || draw.material.color_mask
            || draw.material.state.is_some_and(|state| state.color_mask);
        let dye = (color_mask && draw.tint != 0).then(|| {
            let [r, g, b, _] = draw.tint.to_le_bytes();
            let coverage = (draw.material.color_mask || draw.material.state.is_none())
                && !draw
                    .material
                    .state
                    .is_some_and(|state| state.blend || state.emissive);
            Dye {
                rgb: [r, g, b],
                alpha: if coverage {
                    DyeAlpha::Coverage
                } else {
                    DyeAlpha::Preserve
                },
            }
        });
        let raster = self.trim_raster(
            item,
            artwork,
            location,
            draw.input.rig,
            dye,
            worn.then_some(draw.input.identity),
        )?;
        if color_mask && draw.tint != 0 {
            let alpha = draw.tint >> 24;
            draw.tint = if alpha == 255 {
                0
            } else {
                (alpha << 24) | 0xffffff
            };
            draw.material.color_mask = false;
            // The composed page has no mask. Keep its opacity multiplier without dyeing the trim.
            if draw.tint != 0 {
                if let Some(state) = &mut draw.material.state {
                    state.color_mask = true;
                } else {
                    draw.material.color_mask = true;
                }
            }
        }
        Some(raster)
    }

    /// Decorates the selected armor sheet or held armor sprite, preserving the mesh's UV frame.
    pub(crate) fn trim_raster(
        &mut self,
        item: &WornItem,
        artwork: &ActorArtworkPages,
        location: ActorArtworkLocation,
        rig: EntityRigId,
        dye: Option<Dye>,
        worn: Option<render_model::ActorRenderIdentity>,
    ) -> Option<(EquipmentRaster, EntityRigId)> {
        let trim = item.trim.as_ref()?;
        let slot = assets::item::trim::armor_slot(&item.identifier)?;
        let icon = if worn.is_some() {
            None
        } else {
            match self.meshes.iter().find(|(_, cached)| **cached == Some(rig)) {
                Some((MeshKey::Sprite(index), _)) => Some(*self.placements.get(*index)?.as_ref()?),
                Some((MeshKey::Session(index), _)) => {
                    let (selected, placement, selected_location) =
                        self.session_sprite(&item.identifier, item.metadata)?;
                    if selected != *index || selected_location != location {
                        return None;
                    }
                    Some(placement)
                }
                _ => return None,
            }
        };
        let page = artwork
            .pages()
            .get(usize::from(location.page()).checked_sub(1)?)?;
        let pixels = page.shared_pixels();
        let (width, height) = page.dimensions();
        let (catalog, binding_index, selected_artwork) = if let Some(identity) = worn {
            let selected = self.trims.worn_source(identity)?;
            (
                Arc::clone(&selected.catalog),
                selected.binding,
                Some((Arc::clone(&selected.artwork), selected.texture)),
            )
        } else {
            let (catalog, _) = self.binding_source(&item.identifier)?;
            let index = catalog
                .bindings()
                .partition_point(|binding| binding.identifier.as_ref() <= item.identifier.as_ref())
                .checked_sub(1)?;
            (catalog, index, None)
        };
        let binding = catalog.bindings().get(binding_index)?;
        let (base_path, texture_identity) = if let Some((artwork, index)) = &selected_artwork {
            let texture = artwork.textures().get(*index)?;
            (
                texture.identifier.as_ref(),
                texture as *const assets::EquipmentTexture as usize,
            )
        } else {
            (
                binding.texture.identifier.as_ref(),
                &binding.texture as *const assets::EquipmentReference as usize,
            )
        };
        let key = Key {
            trim: trim.clone(),
            item: Arc::clone(&item.identifier),
            binding: binding as *const assets::EquipmentBinding as usize,
            texture: texture_identity,
            source: pixels.storage_id(),
            layer: location.layer(),
            dye,
            icon: icon.map(|p| [p.x, p.y, p.width, p.height]),
        };
        if !self.trims.images.contains_key(&key) {
            let bytes = usize::from(width) * usize::from(height) * 4;
            let start = usize::try_from(location.layer()).ok()?.checked_mul(bytes)?;
            let selected = pixels.get(start..start.checked_add(bytes)?)?;
            let [x, y, w, h] = key.icon.unwrap_or([0, 0, width, height]);
            if u32::from(x) + u32::from(w) > u32::from(width)
                || u32::from(y) + u32::from(h) > u32::from(height)
            {
                return None;
            }
            let mut base_pixels = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
            for row in y..y + h {
                let start = (usize::from(row) * usize::from(width) + usize::from(x)) * 4;
                base_pixels.extend_from_slice(&selected[start..start + usize::from(w) * 4]);
            }
            let startup = self.catalog.as_deref().unwrap_or(catalog.as_ref());
            let pack = self.pack.as_ref().and_then(|pack| pack.catalog.as_deref());
            let base = assets::EquipmentTexture {
                identifier: base_path.into(),
                width: w,
                height: h,
                rgba8: base_pixels.into(),
            };
            let composed = assets::item::trim::compose(
                &base,
                assets::facts::ItemFacts::builtin()
                    .ok()?
                    .by_identifier(&item.identifier)?
                    .armor_material(),
                &trim.material,
                &trim.pattern,
                slot,
                icon.is_some(),
                dye,
                Some(&binding.resources),
                |path| {
                    pack.and_then(|c| c.texture(path))
                        .or_else(|| startup.texture(path))
                },
            )?;
            let raster = if key.icon.is_none() {
                EquipmentRaster {
                    width: composed.width,
                    height: composed.height,
                    rgba8: composed.rgba8,
                }
            } else {
                let mut output = selected.to_vec();
                for row in 0..h {
                    for column in 0..w {
                        let sx = usize::from(column) * usize::from(composed.width) / usize::from(w);
                        let sy = usize::from(row) * usize::from(composed.height) / usize::from(h);
                        let at = (usize::from(row + y) * usize::from(width)
                            + usize::from(column + x))
                            * 4;
                        let from = (sy * usize::from(composed.width) + sx) * 4;
                        output[at..at + 4].copy_from_slice(&composed.rgba8[from..from + 4]);
                    }
                }
                EquipmentRaster {
                    width,
                    height,
                    rgba8: output.into(),
                }
            };
            self.trims.images.insert(
                key.clone(),
                Image {
                    _source: pixels,
                    _binding: catalog,
                    _artwork: selected_artwork.map(|(catalog, _)| catalog),
                    raster,
                    used: true,
                },
            );
        }
        let image = self.trims.images.get_mut(&key)?;
        image.used = true;
        Some((image.raster.clone(), rig))
    }
}
